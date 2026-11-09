From Stdlib Require Import List Bool Arith Lia NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3Admission
  RPB3WireCursor RPB3GeneratedFieldRefinement RPB3SemanticReducer
  RPB3RawRegionTree RPB3AuxiliaryTypedReference
  RPB3RecursiveAuxiliarySourceReference RPB3SemanticWorklist
  RPB3SourceIndexedFrames EPM1 EPM1ACTree03.

Import ListNotations.
Set Implicit Arguments.

Definition reject_label (label : nat) : bool := negb (label =? 0).

Definition mutant_omit_local (label : nat) (children : list bool) : bool :=
  rpb3_algebra (fun _ => true) label children.

Definition mutant_local_validation (subject : @tree nat) : bool :=
  @fold_tree nat bool mutant_omit_local subject.

Definition valid_local_validation (subject : @tree nat) : bool :=
  @fold_tree nat bool (rpb3_algebra reject_label) subject.

Definition invalid_root : @tree nat := Node 0 FNil.
Definition valid_root : @tree nat := Node 1 FNil.

Theorem mutant_omit_locality_is_refuted :
  mutant_local_validation invalid_root = true /\
  valid_local_validation invalid_root = false.
Proof. split; reflexivity. Qed.

Theorem local_validation_accepts_valid_root :
  valid_local_validation valid_root = true.
Proof. reflexivity. Qed.

Module Protocol := CanonicalKeyProtocol.

Definition trailing_region : Protocol.region :=
  Protocol.Region 7
    [Protocol.LocatedByte 42 3 10; Protocol.LocatedByte 7 3 11] 3 12.

Definition trailing_frame : Protocol.frame :=
  Protocol.Frame Protocol.KeyFrame trailing_region 1
    (Protocol.ReducerState 0 true None).

Definition mutant_skip_root_exhaustion
    (current : Protocol.frame) (parents : list Protocol.suspended)
    (action : Protocol.command) : Protocol.machine * list Protocol.effect :=
  match action with
  | Protocol.Complete _ =>
      Protocol.transport
        (Protocol.Frame (Protocol.frame_kind_of current)
          (Protocol.frame_input current)
          (length (Protocol.region_bytes (Protocol.frame_input current)))
          (Protocol.frame_reducer current))
        parents action
  | _ => Protocol.transport current parents action
  end.

Theorem mutant_trailing_acceptance_is_refuted :
  Protocol.finished trailing_frame (Protocol.KeySummary true) = false /\
  Protocol.transport trailing_frame []
    (Protocol.Complete (Protocol.KeySummary true)) =
    (Protocol.Halted (Protocol.Rejected Protocol.CompletionRefusal),
      [Protocol.Refuse Protocol.CompletionRefusal]) /\
  mutant_skip_root_exhaustion trailing_frame []
    (Protocol.Complete (Protocol.KeySummary true)) =
    (Protocol.Halted (Protocol.Accepted trailing_region (Protocol.KeySummary true)),
      [Protocol.AcceptRoot]).
Proof. repeat split; reflexivity. Qed.

Print Assumptions mutant_omit_locality_is_refuted.
Print Assumptions mutant_trailing_acceptance_is_refuted.

Fixpoint mutant_varint_groups_without_minimality
    (fuel : nat) (bytes : list nat) (region_end offset group value : nat)
    : option (nat * nat) :=
  match fuel with
  | 0 => None
  | S rest =>
      if offset <? region_end then
        match nth_error bytes offset with
        | None => None
        | Some byte =>
            if RPB3WireCursor.byte_ok byte then
              if (group =? 9) && (1 <? byte) then None
              else
                let next_value := value + (byte mod 128) * (128 ^ group) in
                if byte <? 128 then Some (next_value, S offset)
                else mutant_varint_groups_without_minimality rest bytes region_end
                  (S offset) (S group) next_value
            else None
        end
      else None
  end.

Theorem mutant_nonminimal_varint_is_refuted :
  RPB3WireCursor.canonical_varint [128; 0] 2 0 = None /\
  mutant_varint_groups_without_minimality 10 [128; 0] 2 0 0 0 = Some (0, 2).
Proof. vm_compute. split; reflexivity. Qed.

Definition mutant_resolve_oneof_by_slot
    (fields : list RPB3GeneratedFieldRefinement.field_descriptor) (slot : nat)
    : option RPB3GeneratedFieldRefinement.field_descriptor :=
  find (fun field =>
    RPB3GeneratedFieldRefinement.descriptor_slot field =? slot) fields.

Theorem mutant_oneof_arm_tag_binding_is_refuted :
  match RPB3GeneratedFieldRefinement.resolve_tag
      (RPB3GeneratedFieldRefinement.program_fields
        RPB3GeneratedFieldRefinement.expr_program) 2,
    mutant_resolve_oneof_by_slot
      (RPB3GeneratedFieldRefinement.program_fields
        RPB3GeneratedFieldRefinement.expr_program) 0 with
  | Some correct, Some wrong =>
      RPB3GeneratedFieldRefinement.descriptor_tag correct = 2 /\
      RPB3GeneratedFieldRefinement.descriptor_tag wrong = 1
  | _, _ => False
  end.
Proof. vm_compute. split; reflexivity. Qed.

Definition mutant_skip_child_slot_order
    (program : RPB3GeneratedFieldRefinement.generated_program)
    (state : RPB3GeneratedFieldRefinement.scan_state)
    (child : RPB3GeneratedFieldRefinement.typed_summary)
    : RPB3GeneratedFieldRefinement.scan_state :=
  if RPB3GeneratedFieldRefinement.scan_valid state then
    match RPB3GeneratedFieldRefinement.resolve_tag
        (RPB3GeneratedFieldRefinement.program_fields program)
        (RPB3GeneratedFieldRefinement.summary_tag child) with
    | None =>
        RPB3GeneratedFieldRefinement.ScanState false
          (RPB3GeneratedFieldRefinement.scan_last_slot state)
          (RPB3GeneratedFieldRefinement.scan_seen state)
          (RPB3GeneratedFieldRefinement.scan_ordered_slots state)
          (RPB3GeneratedFieldRefinement.scan_ordered_scores state)
    | Some field =>
        let slot := RPB3GeneratedFieldRefinement.descriptor_slot field in
        let admissible :=
          RPB3GeneratedFieldRefinement.summary_valid child &&
          RPB3GeneratedFieldRefinement.wire_eqb
            (RPB3GeneratedFieldRefinement.summary_wire child)
            (RPB3GeneratedFieldRefinement.descriptor_wire field) &&
          RPB3GeneratedFieldRefinement.payload_eqb
            (RPB3GeneratedFieldRefinement.summary_payload child)
            (RPB3GeneratedFieldRefinement.descriptor_payload field) &&
          RPB3GeneratedFieldRefinement.message_child_type_matches field child &&
          (slot <? RPB3GeneratedFieldRefinement.program_slot_count program) &&
          (RPB3GeneratedFieldRefinement.descriptor_repeated field ||
            negb (existsb (Nat.eqb slot)
              (RPB3GeneratedFieldRefinement.scan_seen state))) in
        if admissible then
          RPB3GeneratedFieldRefinement.ScanState true (Some slot)
            (slot :: RPB3GeneratedFieldRefinement.scan_seen state)
            (RPB3GeneratedFieldRefinement.scan_ordered_slots state ++ [slot])
            (RPB3GeneratedFieldRefinement.scan_ordered_scores state ++
              [RPB3GeneratedFieldRefinement.summary_score child])
        else RPB3GeneratedFieldRefinement.ScanState false
          (RPB3GeneratedFieldRefinement.scan_last_slot state)
          (RPB3GeneratedFieldRefinement.scan_seen state)
          (RPB3GeneratedFieldRefinement.scan_ordered_slots state)
          (RPB3GeneratedFieldRefinement.scan_ordered_scores state)
    end
  else state.

Definition decreasing_send_children :
    list RPB3GeneratedFieldRefinement.typed_summary :=
  [RPB3GeneratedFieldRefinement.typed_reference
    (RPB3GeneratedFieldRefinement.raw_child 3
      RPB3GeneratedFieldRefinement.Varint
      RPB3GeneratedFieldRefinement.Scalar);
   RPB3GeneratedFieldRefinement.typed_reference
    (RPB3GeneratedFieldRefinement.raw_child 1
      RPB3GeneratedFieldRefinement.LengthDelimited
      RPB3GeneratedFieldRefinement.Message)].

Theorem mutant_child_slot_order_is_refuted :
  RPB3GeneratedFieldRefinement.scan_valid
    (fold_left
      (RPB3GeneratedFieldRefinement.checked_field
        RPB3GeneratedFieldRefinement.send_program)
      decreasing_send_children RPB3GeneratedFieldRefinement.empty_scan) = false /\
  RPB3GeneratedFieldRefinement.scan_valid
    (fold_left
      (mutant_skip_child_slot_order
        RPB3GeneratedFieldRefinement.send_program)
      decreasing_send_children RPB3GeneratedFieldRefinement.empty_scan) = true.
Proof. vm_compute. split; reflexivity. Qed.

Definition mutant_reverse_score_body_order
    (left right : RPB3SemanticReducer.semantic_summary) : option bool :=
  RPB3SemanticReducer.score_body_order right left.

Theorem mutant_score_body_tiebreak_is_refuted :
  RPB3SemanticReducer.score_body_order
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.first_incomplete_send)
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.empty_incomplete_send) = Some false /\
  mutant_reverse_score_body_order
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.first_incomplete_send)
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.empty_incomplete_send) = Some true.
Proof. vm_compute. split; reflexivity. Qed.

Definition mutant_incomplete_is_refused
    (summary : RPB3SemanticReducer.semantic_summary) : bool :=
  RPB3SemanticReducer.sem_definitely_refused summary ||
    match RPB3SemanticReducer.sem_complete summary with
    | Some false => true
    | _ => false
    end.

Theorem mutant_incomplete_lane_is_refuted :
  RPB3SemanticReducer.sem_complete
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.empty_incomplete_send) = Some false /\
  RPB3SemanticReducer.sem_definitely_refused
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.empty_incomplete_send) = false /\
  mutant_incomplete_is_refused
    (RPB3SemanticReducer.semantic_reduce
      RPB3SemanticReducer.empty_incomplete_send) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions mutant_nonminimal_varint_is_refuted.
Print Assumptions mutant_oneof_arm_tag_binding_is_refuted.
Print Assumptions mutant_child_slot_order_is_refuted.
Print Assumptions mutant_score_body_tiebreak_is_refuted.
Print Assumptions mutant_incomplete_lane_is_refuted.

Definition mutant_force_positive_score_sign (bits : N) : list nat :=
  [1; 1; 128 + N.to_nat (N.land (N.shiftr bits 56) 127)] ++
    skipn 3 (RPB3SemanticReducer.score_integer_bytes bits).

Theorem mutant_forced_positive_score_sign_is_refuted :
  RPB3SemanticReducer.score_integer_bytes
    (RPB3SemanticReducer.signed_score_word (-1)) <>
  mutant_force_positive_score_sign
    (RPB3SemanticReducer.signed_score_word (-1)).
Proof. vm_compute. discriminate. Qed.

Definition mutant_unescaped_string_score (bytes : list nat) : list nat :=
  [1; 2] ++ bytes ++ [0; 0].

Theorem mutant_unescaped_string_score_is_refuted :
  RPB3SemanticReducer.canonical_score_bytes
    (RPB3SemanticReducer.ScoreString [97; 0; 98]) <>
  mutant_unescaped_string_score [97; 0; 98].
Proof. vm_compute. discriminate. Qed.

Definition mutant_zipper_borrows_nested_connective
    (snapshot : list nat) (nested_flag own_flag : bool)
    : RPB3SemanticReducer.canonical_score :=
  RPB3SemanticReducer.score_constructor 14
    [RPB3SemanticReducer.ScoreBytes snapshot;
     RPB3SemanticReducer.ScoreInteger (if nested_flag then 1 else 0)].

Theorem mutant_zipper_nested_connective_is_refuted :
  RPB3SemanticReducer.canonical_score_bytes
    (RPB3SemanticReducer.score_constructor 14
      [RPB3SemanticReducer.ScoreBytes [0; 1; 255];
       RPB3SemanticReducer.ScoreInteger 0]) <>
  RPB3SemanticReducer.canonical_score_bytes
    (mutant_zipper_borrows_nested_connective [0; 1; 255] true false).
Proof. vm_compute. discriminate. Qed.

Print Assumptions mutant_forced_positive_score_sign_is_refuted.
Print Assumptions mutant_unescaped_string_score_is_refuted.
Print Assumptions mutant_zipper_nested_connective_is_refuted.

Definition send_slot (tag : nat) : option nat :=
  option_map RPB3GeneratedFieldRefinement.descriptor_slot
    (RPB3GeneratedFieldRefinement.resolve_tag
      (RPB3GeneratedFieldRefinement.program_fields
        RPB3GeneratedFieldRefinement.send_program) tag).

Theorem generated_slot_order_implies_ordered_bucket_precondition :
  forall previous slot repeated,
    RPB3GeneratedFieldRefinement.slot_follows previous slot repeated = true ->
    OrderedSemanticSlots.follows previous slot = true.
Proof.
  intros [last|] slot repeated checked; [|reflexivity].
  unfold RPB3GeneratedFieldRefinement.slot_follows in checked.
  unfold OrderedSemanticSlots.follows.
  apply Nat.leb_le.
  apply orb_true_iff in checked as [strict|same].
  - apply Nat.ltb_lt in strict. lia.
  - apply andb_true_iff in same as [equal _].
    apply Nat.eqb_eq in equal. lia.
Qed.

(** Sorting completed children into the per-slot vectors while forgetting the
    checked arrival cursor changes the source order of a decreasing Send pair.
    The legal indexed accumulator refuses that pair before any reduction. *)
Definition mutant_unchecked_slot_flatten : list nat :=
  match send_slot 3, send_slot 1 with
  | Some later, Some earlier =>
      OrderedSemanticSlots.flatten
        (RPB3GeneratedFieldRefinement.program_slot_count
          RPB3GeneratedFieldRefinement.send_program)
        (OrderedSemanticSlots.append_at
          (OrderedSemanticSlots.append_at
            OrderedSemanticSlots.empty_nat_buckets later 3) earlier 1)
  | _, _ => []
  end.

Theorem mutant_unchecked_slot_flatten_is_refuted :
  send_slot 3 = Some 2 /\
  send_slot 1 = Some 0 /\
  (match send_slot 3, send_slot 1 with
   | Some later, Some earlier =>
       OrderedSemanticSlots.accept
         (RPB3GeneratedFieldRefinement.program_slot_count
           RPB3GeneratedFieldRefinement.send_program)
         [(later, 3); (earlier, 1)]
         (None, OrderedSemanticSlots.empty_nat_buckets)
   | _, _ => None
   end) = None /\
  mutant_unchecked_slot_flatten = [1; 3] /\
  mutant_unchecked_slot_flatten <> [3; 1].
Proof. vm_compute. repeat split; try reflexivity; discriminate. Qed.

Print Assumptions mutant_unchecked_slot_flatten_is_refuted.
Print Assumptions generated_slot_order_implies_ordered_bucket_precondition.

(** A decoded field keeps its actual wire tag and selected generated descriptor.
    Convert each field to the independent typed reference before asking the
    generated scanner to accept it. The bucket worklist below consumes those
    same source-ordered events, rather than a caller-selected slot sequence. *)
Module DecodedSourceOrder.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Wire := RPB3WireCursor.
Module Slots := OrderedSemanticSlots.

Fixpoint typed_events (fuel : nat) (bytes : list nat)
    (nodes : list Source.source_tree)
    : option (list (nat * Schema.typed_summary)) :=
  match nodes with
  | [] => Some []
  | node :: rest =>
      match Source.raw_tree_of_source fuel bytes node,
          typed_events fuel bytes rest with
      | Some raw, Some tail =>
          Some ((Schema.descriptor_slot (Source.source_field node),
            Schema.typed_reference raw) :: tail)
      | _, _ => None
      end
  end.

Lemma converted_field_has_actual_wire_tag :
  forall fuel bytes node raw,
    Source.raw_tree_of_source fuel bytes node = Some raw ->
    Schema.summary_tag (Schema.typed_reference raw) =
      Wire.span_number (Source.source_span node).
Proof.
  intros fuel bytes node raw converted.
  destruct (Source.raw_tree_of_source_preserves_root_label
    fuel bytes node raw converted) as [label [children [shape raw_field]]].
  subst raw. cbn [Schema.typed_reference Schema.reduce_field Schema.summary_tag].
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    bytes node label raw_field) as [actual_tag _].
  exact actual_tag.
Qed.

Lemma invalid_generated_scan_stays_invalid :
  forall program children state,
    Schema.scan_valid state = false ->
    Schema.scan_valid
      (fold_left (Schema.checked_field program) children state) = false.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state invalid;
    simpl; [exact invalid|].
  apply IH. unfold Schema.checked_field.
  now rewrite invalid.
Qed.

Lemma accepted_decoded_field_advances_slot :
  forall program fuel bytes node raw state,
    Source.source_node_matches_generated_descriptor program node ->
    Source.raw_tree_of_source fuel bytes node = Some raw ->
    Schema.scan_valid state = true ->
    Schema.scan_valid
      (Schema.checked_field program state (Schema.typed_reference raw)) = true ->
    let slot := Schema.descriptor_slot (Source.source_field node) in
    slot < Schema.program_slot_count program /\
    Slots.follows (Schema.scan_last_slot state) slot = true /\
    Schema.scan_last_slot
      (Schema.checked_field program state (Schema.typed_reference raw)) =
      Some slot.
Proof.
  intros program fuel bytes node raw state [selected _] converted
    valid accepted slot.
  pose proof (@converted_field_has_actual_wire_tag
    fuel bytes node raw converted) as actual_tag.
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Wire.span_number (Source.source_span node))) as [field|]
    eqn:resolved; [|discriminate].
  destruct (Schema.wire_eqb
    (Source.wire_kind_of_span (Source.source_span node))
    (Schema.descriptor_wire field)); [|discriminate].
  inversion selected; subst field; clear selected.
  unfold Schema.checked_field in accepted |- *.
  rewrite valid, actual_tag, resolved in accepted |- *.
  unfold slot in *.
  remember (Schema.summary_valid (Schema.typed_reference raw) &&
    Schema.wire_eqb (Schema.summary_wire (Schema.typed_reference raw))
      (Schema.descriptor_wire (Source.source_field node)) &&
    Schema.payload_eqb (Schema.summary_payload (Schema.typed_reference raw))
      (Schema.descriptor_payload (Source.source_field node)) &&
    Schema.message_child_type_matches (Source.source_field node)
      (Schema.typed_reference raw) &&
    (Schema.descriptor_slot (Source.source_field node) <?
      Schema.program_slot_count program) &&
    Schema.slot_follows (Schema.scan_last_slot state)
      (Schema.descriptor_slot (Source.source_field node))
      (Schema.descriptor_repeated (Source.source_field node)) &&
    (Schema.descriptor_repeated (Source.source_field node) ||
      negb (existsb
        (Nat.eqb (Schema.descriptor_slot (Source.source_field node)))
        (Schema.scan_seen state))))
    as admissible eqn:admitted.
  destruct admissible; [|discriminate].
  symmetry in admitted.
  repeat rewrite andb_true_iff in admitted.
  destruct admitted as [[[[[[valid_child wire_ok] payload_ok]
    child_type_ok] within] ordered] unique].
  split.
  - now apply Nat.ltb_lt.
  - split.
    + eapply generated_slot_order_implies_ordered_bucket_precondition.
      exact ordered.
    + reflexivity.
Qed.

Theorem accepted_decoded_fields_preserve_typed_source_order :
  forall program fuel bytes nodes events state saved,
    Forall (Source.source_node_matches_generated_descriptor program) nodes ->
    typed_events fuel bytes nodes = Some events ->
    Schema.scan_valid state = true ->
    Slots.empty_after (Schema.scan_last_slot state) saved ->
    Schema.scan_valid
      (fold_left (Schema.checked_field program) (map snd events) state) = true ->
    exists final_saved,
      Slots.accept (Schema.program_slot_count program) events
        (Schema.scan_last_slot state, saved) =
        Some (Schema.scan_last_slot
          (fold_left (Schema.checked_field program) (map snd events) state),
          final_saved) /\
      Slots.empty_after
        (Schema.scan_last_slot
          (fold_left (Schema.checked_field program) (map snd events) state))
        final_saved /\
      Slots.flatten (Schema.program_slot_count program) final_saved =
        Slots.flatten (Schema.program_slot_count program) saved ++
          map snd events.
Proof.
  intros program fuel bytes nodes.
  induction nodes as [|node rest IH];
    intros events state saved matched converted valid empty accepted.
  - simpl in converted. inversion converted; subst events.
    exists saved. simpl. split; [reflexivity|].
    split; [exact empty|].
    now rewrite app_nil_r.
  - inversion matched as [|head tail head_match tail_match]; subst.
    simpl in converted.
    destruct (Source.raw_tree_of_source fuel bytes node)
      as [raw|] eqn:raw_result; [|discriminate].
    destruct (typed_events fuel bytes rest) as [tail_events|]
      eqn:tail_result; [|discriminate].
    inversion converted; subst events; clear converted.
    simpl [map fold_left] in accepted |- *.
    set (child := Schema.typed_reference raw) in *.
    set (next := Schema.checked_field program state child) in *.
    assert (next_valid : Schema.scan_valid next = true).
    { destruct (Schema.scan_valid next) eqn:next_status;
        [reflexivity|].
      pose proof (invalid_generated_scan_stays_invalid
        program (map snd tail_events) next next_status) as rejected.
      rewrite accepted in rejected. discriminate. }
    destruct (@accepted_decoded_field_advances_slot
      program fuel bytes node raw state head_match raw_result
      valid next_valid) as [within [follows next_slot]].
    fold child next in next_slot.
    assert (empty_above : forall index,
        Schema.descriptor_slot (Source.source_field node) < index ->
        saved index = []).
    { eapply (@Slots.empty_after_follows Schema.typed_summary
        (Schema.scan_last_slot state) saved
        (Schema.descriptor_slot (Source.source_field node)));
        [exact empty|exact follows]. }
    assert (empty_next : Slots.empty_after
        (Schema.scan_last_slot next)
        (Slots.append_at saved
          (Schema.descriptor_slot (Source.source_field node)) child)).
    { rewrite next_slot.
      intros index later.
      exact (@Slots.append_at_empty_after Schema.typed_summary saved
        (Schema.descriptor_slot (Source.source_field node)) child
        empty_above index later). }
    destruct (IH tail_events next
      (Slots.append_at saved
        (Schema.descriptor_slot (Source.source_field node)) child)
      tail_match eq_refl next_valid empty_next accepted)
      as [final_saved [rest_accept [final_empty final_order]]].
    exists final_saved. split.
    + simpl [Slots.accept].
      assert (within_bool :
        (Schema.descriptor_slot (Source.source_field node) <?
          Schema.program_slot_count program) = true).
      { now apply Nat.ltb_lt. }
      rewrite within_bool, follows. simpl.
      rewrite <- next_slot. exact rest_accept.
    + split; [exact final_empty|].
      rewrite final_order.
      rewrite (@Slots.flatten_append_at Schema.typed_summary
        (Schema.program_slot_count program) saved
        (Schema.descriptor_slot (Source.source_field node)) child within).
      * simpl [map]. now rewrite <- app_assoc.
      * exact empty_above.
Qed.

Theorem decoded_raw_wire_fields_preserve_reference_order :
  forall fuel bytes program region_start region_end nodes events,
    Source.decode_region fuel bytes program region_start region_end =
      Some nodes ->
    typed_events (S (length bytes)) bytes nodes = Some events ->
    Schema.scan_valid (fold_left (Schema.checked_field program)
      (map snd events) Schema.empty_scan) = true ->
    exists final_saved,
      Slots.accept (Schema.program_slot_count program) events
        (None, fun _ => []) =
        Some (Schema.scan_last_slot
          (fold_left (Schema.checked_field program)
            (map snd events) Schema.empty_scan), final_saved) /\
      Slots.flatten (Schema.program_slot_count program) final_saved =
        map snd events.
Proof.
  intros fuel bytes program region_start region_end nodes events
    decoded converted accepted.
  pose proof (@Source.decoded_region_nodes_match_generated_source
    fuel bytes program region_start region_end nodes decoded) as matched.
  destruct (@accepted_decoded_fields_preserve_typed_source_order
    program (S (length bytes)) bytes nodes events Schema.empty_scan
    (fun _ => []) matched converted eq_refl
    (fun _ _ => eq_refl) accepted)
    as [final_saved [ordered [_ flattened]]].
  exists final_saved. split; [exact ordered|].
  assert (empty_flatten :
      Slots.flatten (Schema.program_slot_count program)
        (fun _ : nat => ([] : list Schema.typed_summary)) = []).
  { unfold Slots.flatten.
    apply (@Slots.flat_map_empty_suffix Schema.typed_summary 0
      (Schema.program_slot_count program) (fun _ => [])).
    intros index later. reflexivity. }
  rewrite empty_flatten in flattened.
  simpl in flattened. exact flattened.
Qed.

Print Assumptions decoded_raw_wire_fields_preserve_reference_order.
End DecodedSourceOrder.

Module SourceAuxiliaryOrder.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Worklist := RPB3SemanticWorklist.
Module Wire := RPB3WireCursor.
Module Slots := OrderedSemanticSlots.

Definition source_slot_events (nodes : list Source.source_tree)
    : list (nat * Source.source_tree) :=
  map (fun node =>
    (Schema.descriptor_slot (Source.source_field node), node)) nodes.

Lemma source_slot_events_snoc : forall nodes node,
  source_slot_events (nodes ++ [node]) =
    source_slot_events nodes ++
      [(Schema.descriptor_slot (Source.source_field node), node)].
Proof.
  intros nodes node.
  unfold source_slot_events.
  now rewrite map_app.
Qed.

Definition empty_source_buckets : Slots.buckets Source.source_tree :=
  fun _ => [].

Definition source_trace_invariant (program : Schema.generated_program)
    (current : Source.decode_accumulator) : Prop :=
  exists saved,
    Slots.accept (Schema.program_slot_count program)
      (source_slot_events (rev (Source.decode_reversed current)))
      (None, empty_source_buckets) =
      Some (Source.decode_last_slot current, saved) /\
    Slots.empty_after (Source.decode_last_slot current) saved.

Lemma accept_app :
  forall (A : Type) count before after current middle,
    @Slots.accept A count before current = Some middle ->
    @Slots.accept A count (before ++ after) current =
      @Slots.accept A count after middle.
Proof.
  intros A count before.
  induction before as [|[slot value] rest IH];
    intros after current middle accepted; simpl in *.
  - now inversion accepted; subst middle.
  - destruct ((slot <? count) && Slots.follows (fst current) slot);
      [|discriminate].
    eapply IH. exact accepted.
Qed.

Lemma decode_step_preserves_source_trace :
  forall program decode_child current span next,
    Source.decode_source_step program decode_child (Some current) span =
      Some next ->
    source_trace_invariant program current ->
    source_trace_invariant program next.
Proof.
  intros program decode_child current span next advanced
    [saved [prior_trace prior_empty]].
  unfold Source.decode_source_step in advanced.
  destruct (Source.source_descriptor program span) as [field|]
    eqn:selected; [|discriminate].
  destruct (Source.slot_admissible program current field)
    eqn:admitted; [|discriminate].
  destruct (Source.generated_source_node decode_child span field)
    as [node|] eqn:generated; [|discriminate].
  inversion advanced; subst next; clear advanced.
  destruct (Source.generated_source_node_has_selected_constructor
    decode_child span field node generated) as [_ [same_field _]].
  destruct (Source.admitted_source_slot_obeys_generated_order
    program current field admitted) as [within [ordered _]].
  pose proof (@generated_slot_order_implies_ordered_bucket_precondition
    (Source.decode_last_slot current) (Schema.descriptor_slot field)
    (Schema.descriptor_repeated field) ordered) as follows.
  assert (empty_above : forall index,
      Schema.descriptor_slot field < index -> saved index = []).
  { eapply (@Slots.empty_after_follows Source.source_tree
      (Source.decode_last_slot current) saved
      (Schema.descriptor_slot field));
      [exact prior_empty|exact follows]. }
  exists (Slots.append_at saved (Schema.descriptor_slot field) node).
  split.
  - change (Slots.accept (Schema.program_slot_count program)
      (source_slot_events
        (rev (Source.decode_reversed current) ++ [node]))
      (None, empty_source_buckets) =
      Some (Some (Schema.descriptor_slot field),
        Slots.append_at saved (Schema.descriptor_slot field) node)).
    rewrite source_slot_events_snoc.
    rewrite same_field.
    rewrite (@accept_app Source.source_tree
      (Schema.program_slot_count program)
      (source_slot_events (rev (Source.decode_reversed current)))
      [(Schema.descriptor_slot field, node)]
      (None, empty_source_buckets)
      (Source.decode_last_slot current, saved) prior_trace).
    simpl [Slots.accept].
    assert (within_bool :
      (Schema.descriptor_slot field <?
        Schema.program_slot_count program) = true).
    { now apply Nat.ltb_lt. }
    now rewrite within_bool, follows.
  - intros index later.
    exact (@Slots.append_at_empty_after Source.source_tree saved
      (Schema.descriptor_slot field) node empty_above index later).
Qed.

Theorem decoded_source_slots_are_accepted :
  forall fuel bytes program region_start region_end nodes,
    Source.decode_region fuel bytes program region_start region_end =
      Some nodes ->
    exists last saved,
      Slots.accept (Schema.program_slot_count program)
        (source_slot_events nodes) (None, empty_source_buckets) =
        Some (last, saved).
Proof.
  intros [|smaller] bytes program region_start region_end nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate.
  set (decode_child := fun (child_program : Schema.generated_program)
    (start finish : nat) =>
    Source.decode_region smaller bytes child_program start finish).
  change (option_map (fun state => rev (Source.decode_reversed state))
    (fold_left (Source.decode_source_step program decode_child) spans
      (Some (Source.DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (Source.decode_source_step program decode_child)
    spans (Some (Source.DecodeAccumulator None [] [])))
    as [final|] eqn:folded; [|discriminate].
  inversion decoded; subst nodes.
  assert (final_trace : source_trace_invariant program final).
  { eapply Source.successful_option_fold_preserves_invariant with
      (step := Source.decode_source_step program decode_child)
      (invariant := source_trace_invariant program)
      (initial := Source.DecodeAccumulator None [] []);
      [| | |exact folded].
    - intro span. reflexivity.
    - intros current span next advanced previous.
      eapply decode_step_preserves_source_trace;
        [exact advanced|exact previous].
    - exists empty_source_buckets.
      split; [reflexivity|].
      intros index unused. reflexivity. }
  destruct final_trace as [saved [accepted _]].
  exists (Source.decode_last_slot final), saved.
  exact accepted.
Qed.

Inductive reference_value : Type :=
| OrdinaryValue (summary : Schema.typed_summary)
| AuxiliaryValue (result : Auxiliary.auxiliary_result).

Definition field_reference (hash_fn : list nat -> nat)
    (bytes : list nat) (program : Schema.generated_program)
    (node : Source.source_tree) : option reference_value :=
  match node with
  | Source.SourceAuxiliary span field =>
      option_map AuxiliaryValue
        (Auxiliary.typed_auxiliary_reference hash_fn bytes program
          (Schema.descriptor_slot field)
          (Source.SourceAuxiliary span field))
  | _ =>
      option_map (fun raw => OrdinaryValue (Schema.typed_reference raw))
        (Source.raw_tree_of_source (S (length bytes)) bytes node)
  end.

Theorem auxiliary_field_reference_has_exact_source_leaf :
  forall hash_fn bytes program node result,
    field_reference hash_fn bytes program node =
      Some (AuxiliaryValue result) ->
    exists span field raw,
      node = Source.SourceAuxiliary span field /\
      Source.source_descriptor program span = Some field /\
      Source.raw_field_of_source bytes node = Some raw /\
      Schema.valid_raw_body raw = true /\
      Auxiliary.auxiliary_result_raw result = raw /\
      Source.raw_tree_of_source (S (length bytes)) bytes node =
        Some (Node raw FNil).
Proof.
  intros hash_fn bytes program node result selected.
  destruct node as [span field|span field children|span field].
  - change (option_map
      (fun raw => OrdinaryValue (Schema.typed_reference raw))
      (Source.raw_tree_of_source (S (length bytes)) bytes
        (Source.SourceScalar span field)) =
      Some (AuxiliaryValue result)) in selected.
    destruct (Source.raw_tree_of_source (S (length bytes)) bytes
      (Source.SourceScalar span field)) as [raw|];
      simpl in selected; discriminate.
  - change (option_map
      (fun raw => OrdinaryValue (Schema.typed_reference raw))
      (Source.raw_tree_of_source (S (length bytes)) bytes
        (Source.SourceMessage span field children)) =
      Some (AuxiliaryValue result)) in selected.
    destruct (Source.raw_tree_of_source (S (length bytes)) bytes
      (Source.SourceMessage span field children)) as [raw|];
      simpl in selected; discriminate.
  - change (option_map AuxiliaryValue
      (Auxiliary.typed_auxiliary_reference hash_fn bytes program
        (Schema.descriptor_slot field) (Source.SourceAuxiliary span field)) =
      Some (AuxiliaryValue result)) in selected.
    destruct (Auxiliary.typed_auxiliary_reference hash_fn bytes program
      (Schema.descriptor_slot field) (Source.SourceAuxiliary span field))
      as [accepted|] eqn:reference; simpl in selected; [|discriminate].
    inversion selected; subst accepted.
    destruct (Auxiliary.accepted_auxiliary_has_exact_generated_raw_leaf
      hash_fn bytes program (Schema.descriptor_slot field)
      (Source.SourceAuxiliary span field) result reference)
      as [actual_span [actual_field [raw
        [shape [generated [_ [converted [valid [same [leaf _]]]]]]]]]].
    inversion shape; subst actual_span actual_field.
    exists span, field, raw.
    now repeat split.
Qed.

Fixpoint reference_events (hash_fn : list nat -> nat)
    (bytes : list nat) (program : Schema.generated_program)
    (nodes : list Source.source_tree)
    : option (list (nat * reference_value)) :=
  match nodes with
  | [] => Some []
  | node :: rest =>
      match field_reference hash_fn bytes program node,
          reference_events hash_fn bytes program rest with
      | Some value, Some tail =>
          Some ((Schema.descriptor_slot (Source.source_field node), value)
            :: tail)
      | _, _ => None
      end
  end.

Lemma reference_events_retain_source_slots :
  forall hash_fn bytes program nodes events,
    reference_events hash_fn bytes program nodes = Some events ->
    map fst events = map fst (source_slot_events nodes) /\
    Forall2 (fun node value =>
      field_reference hash_fn bytes program node = Some value)
      nodes (map snd events).
Proof.
  intros hash_fn bytes program nodes.
  induction nodes as [|node rest IH]; intros events returned.
  - simpl in returned. inversion returned; subst events.
    split; [reflexivity|constructor].
  - simpl in returned.
    destruct (field_reference hash_fn bytes program node)
      as [value|] eqn:head; [|discriminate].
    destruct (reference_events hash_fn bytes program rest)
      as [tail|] eqn:continued; [|discriminate].
    inversion returned; subst events.
    destruct (IH tail eq_refl) as [slots positional].
    split.
    + simpl. now rewrite slots.
    + constructor; [exact head|exact positional].
Qed.

Lemma accept_relabels_values :
  forall (A B : Type) count original relabeled last
      saved_original saved_relabeled final_last final_original,
    map fst original = map fst relabeled ->
    @Slots.accept A count original (last, saved_original) =
      Some (final_last, final_original) ->
    exists final_relabeled,
      @Slots.accept B count relabeled (last, saved_relabeled) =
        Some (final_last, final_relabeled).
Proof.
  intros A B count original.
  induction original as [|[slot value] rest IH];
    intros relabeled last saved_original saved_relabeled
      final_last final_original same accepted.
  - destruct relabeled as [|[other other_value] tail];
      simpl in same; [|discriminate].
    simpl in accepted. inversion accepted; subst.
    exists saved_relabeled. reflexivity.
  - destruct relabeled as [|[other other_value] tail];
      simpl in same; [discriminate|].
    assert (same_slot : slot = other) by (injection same; auto).
    assert (same_tail : map fst rest = map fst tail)
      by (injection same; auto).
    subst other.
    simpl in accepted.
    destruct ((slot <? count) && Slots.follows last slot)
      eqn:allowed; [|discriminate].
    specialize (IH tail (Some slot)
      (Slots.append_at saved_original slot value)
      (Slots.append_at saved_relabeled slot other_value)
      final_last final_original same_tail accepted).
    destruct IH as [final_relabeled result].
    exists final_relabeled. simpl.
    now rewrite allowed.
Qed.

Theorem decoded_source_auxiliary_references_keep_wire_order :
  forall fuel bytes program region_start region_end nodes
      hash_fn events,
    Source.decode_region fuel bytes program region_start region_end =
      Some nodes ->
    reference_events hash_fn bytes program nodes = Some events ->
    exists last saved,
      Slots.accept (Schema.program_slot_count program) events
        (None, fun _ => []) = Some (last, saved) /\
      Slots.flatten (Schema.program_slot_count program) saved =
        map snd events /\
      Forall2 (fun node value =>
        field_reference hash_fn bytes program node = Some value)
        nodes (map snd events).
Proof.
  intros fuel bytes program region_start region_end nodes
    hash_fn events decoded converted.
  destruct (@decoded_source_slots_are_accepted
    fuel bytes program region_start region_end nodes decoded)
    as [last [source_saved source_accepted]].
  destruct (@reference_events_retain_source_slots
    hash_fn bytes program nodes events converted)
    as [same_slots positional].
  destruct (@accept_relabels_values Source.source_tree reference_value
    (Schema.program_slot_count program)
    (source_slot_events nodes) events None empty_source_buckets
    (fun _ => []) last source_saved (eq_sym same_slots)
    source_accepted) as [saved accepted].
  exists last, saved. split; [exact accepted|].
  split; [|exact positional].
  destruct (@Slots.accepted_occurrences_keep_exact_source_order
    reference_value (Schema.program_slot_count program) events None
    (fun _ => []) last saved (fun _ _ => eq_refl) accepted)
    as [_ ordered].
  assert (empty_flatten :
      Slots.flatten (Schema.program_slot_count program)
        (fun _ : nat => ([] : list reference_value)) = []).
  { unfold Slots.flatten.
    apply (@Slots.flat_map_empty_suffix reference_value 0
      (Schema.program_slot_count program) (fun _ => [])).
    intros index later. reflexivity. }
  now rewrite empty_flatten in ordered.
Qed.

Definition recursive_semantic_events
    (nodes : list Source.source_tree)
    (results : list Recursive.recursive_mixed_result)
    : list (nat * RPB3SemanticReducer.semantic_summary) :=
  combine
    (map (fun node =>
      Schema.descriptor_slot (Source.source_field node)) nodes)
    (map Recursive.recursive_result_semantic results).

Lemma aligned_recursive_semantics_keep_source_slots :
  forall hash_fn bytes program nodes results,
    Forall2 (fun node result =>
      Recursive.recursive_source_reference
        (Recursive.recursive_par_reference hash_fn)
        hash_fn bytes program node = Some result) nodes results ->
    map fst (recursive_semantic_events nodes results) =
      map fst (source_slot_events nodes) /\
    map snd (recursive_semantic_events nodes results) =
      map Recursive.recursive_result_semantic results.
Proof.
  intros hash_fn bytes program nodes results aligned.
  induction aligned as [|node result nodes results selected aligned IH].
  - split; reflexivity.
  - destruct IH as [slots semantics].
    split.
    + change
        (Schema.descriptor_slot (Source.source_field node) ::
          map fst (recursive_semantic_events nodes results) =
         Schema.descriptor_slot (Source.source_field node) ::
          map fst (source_slot_events nodes)).
      now rewrite slots.
    + change
        (Recursive.recursive_result_semantic result ::
          map snd (recursive_semantic_events nodes results) =
         Recursive.recursive_result_semantic result ::
          map Recursive.recursive_result_semantic results).
      now rewrite semantics.
Qed.

Theorem accepted_recursive_root_semantics_keep_generated_source_order :
  forall hash_fn type_id bytes results,
    Recursive.recursive_mixed_root_reference hash_fn type_id bytes =
      Some results ->
    exists program nodes last saved,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      Forall2 (fun node result =>
        Recursive.recursive_source_reference
          (Recursive.recursive_par_reference hash_fn)
          hash_fn bytes program node = Some result) nodes results /\
      Slots.accept (Schema.program_slot_count program)
        (recursive_semantic_events nodes results)
        (None, fun _ => []) = Some (last, saved) /\
      Slots.flatten (Schema.program_slot_count program) saved =
        map Recursive.recursive_result_semantic results.
Proof.
  intros hash_fn type_id bytes results accepted_root.
  unfold Recursive.recursive_mixed_root_reference in accepted_root.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_root type_id bytes)
    as [nodes|] eqn:decoded; [|discriminate].
  assert (decoded_region : Source.decode_region (S (length bytes))
      bytes program 0 (length bytes) = Some nodes).
  { unfold Source.decode_root in decoded.
    now rewrite looked_up in decoded. }
  pose proof (@Recursive.accepted_recursive_children_preserve_source_order
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program
    nodes results accepted_root) as aligned.
  destruct (@decoded_source_slots_are_accepted
    (S (length bytes)) bytes program 0 (length bytes)
    nodes decoded_region) as [last [source_saved source_accepted]].
  destruct (@aligned_recursive_semantics_keep_source_slots
    hash_fn bytes program nodes results aligned)
    as [same_slots same_semantics].
  destruct (@accept_relabels_values Source.source_tree
    RPB3SemanticReducer.semantic_summary
    (Schema.program_slot_count program)
    (source_slot_events nodes) (recursive_semantic_events nodes results)
    None empty_source_buckets (fun _ => []) last source_saved
    (eq_sym same_slots) source_accepted) as [saved semantic_accepted].
  exists program, nodes, last, saved.
  repeat split; try assumption.
  destruct (@Slots.accepted_occurrences_keep_exact_source_order
    RPB3SemanticReducer.semantic_summary
    (Schema.program_slot_count program)
    (recursive_semantic_events nodes results) None
    (fun _ => []) last saved (fun _ _ => eq_refl) semantic_accepted)
    as [_ ordered].
  assert (empty_flatten :
      Slots.flatten (Schema.program_slot_count program)
        (fun _ : nat => ([] : list RPB3SemanticReducer.semantic_summary)) = []).
  { unfold Slots.flatten.
    apply (@Slots.flat_map_empty_suffix
      RPB3SemanticReducer.semantic_summary 0
      (Schema.program_slot_count program) (fun _ => [])).
    intros index later. reflexivity. }
  rewrite empty_flatten in ordered.
  simpl in ordered.
  now rewrite same_semantics in ordered.
Qed.

Definition source_generated_root_field (type_id : nat)
    (bytes : list nat) : Schema.raw_field :=
  SourceIndexedFrames.source_generated_root_field type_id bytes.

Theorem accepted_recursive_root_has_exact_explicit_close_summary :
  forall hash_fn type_id bytes results,
    Recursive.recursive_mixed_root_reference hash_fn type_id bytes =
      Some results ->
    exists program nodes last saved,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      Slots.accept (Schema.program_slot_count program)
        (recursive_semantic_events nodes results)
        (None, fun _ => []) = Some (last, saved) /\
      Slots.flatten (Schema.program_slot_count program) saved =
        map Recursive.recursive_result_semantic results /\
      Worklist.run_semantic_frames 1
        [Worklist.Close (source_generated_root_field type_id bytes)
          (length results)]
        (rev (Slots.flatten (Schema.program_slot_count program) saved)) =
      Some [RPB3SemanticReducer.reduce_semantic_field
        (source_generated_root_field type_id bytes)
        (map Recursive.recursive_result_semantic results)].
Proof.
  intros hash_fn type_id bytes results accepted.
  destruct (@accepted_recursive_root_semantics_keep_generated_source_order
    hash_fn type_id bytes results accepted) as
    [program [nodes [last [saved
      [looked_up [decoded [_ [admitted ordered]]]]]]]].
  exists program, nodes, last, saved.
  repeat split; try assumption.
  rewrite ordered.
  replace (length results)
    with (length (map Recursive.recursive_result_semantic results))
    by now rewrite map_length.
  replace (rev (map Recursive.recursive_result_semantic results))
    with (rev (map Recursive.recursive_result_semantic results) ++ [])
    by now rewrite app_nil_r.
  rewrite Worklist.close_frame_restores_exact_source_child_order.
  reflexivity.
Qed.

Definition source_auxiliary_visit_transition
    (par_child : list nat -> option (bool * RPB3SemanticReducer.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (node : Source.source_tree) (rest : list Worklist.semantic_job)
    (values : list RPB3SemanticReducer.semantic_summary)
    : option
      ((list Worklist.semantic_job *
          list RPB3SemanticReducer.semantic_summary) *
       (list Worklist.semantic_job *
          list RPB3SemanticReducer.semantic_summary)) :=
  match Source.raw_tree_of_source (S (length bytes)) bytes node,
      Recursive.recursive_auxiliary_reference par_child hash_fn bytes
        program slot node with
  | Some (Node raw FNil),
      Some (Recursive.RecursiveAuxiliaryResult _ semantic) =>
      Some ((Worklist.Visit (Node raw FNil) :: rest, values),
        (rest, semantic :: values))
  | _, _ => None
  end.

Theorem accepted_recursive_auxiliary_source_visit_delivers_exact_cut :
  forall par_child hash_fn bytes program span field
      auxiliary semantic rest values,
    Recursive.recursive_auxiliary_reference par_child hash_fn bytes
      program (Schema.descriptor_slot field)
      (Source.SourceAuxiliary span field) =
      Some (Recursive.RecursiveAuxiliaryResult auxiliary semantic) ->
    exists raw,
      Recursive.checked_generated_raw bytes program
        (Source.SourceAuxiliary span field) = Some raw /\
      Source.raw_tree_of_source (S (length bytes)) bytes
        (Source.SourceAuxiliary span field) = Some (Node raw FNil) /\
      raw = Recursive.recursive_auxiliary_raw auxiliary /\
      Recursive.recursive_auxiliary_semantic auxiliary = Some semantic /\
      source_auxiliary_visit_transition par_child hash_fn bytes program
        (Schema.descriptor_slot field)
        (Source.SourceAuxiliary span field) rest values =
      Some ((Worklist.Visit (Node raw FNil) :: rest, values),
        (rest, semantic :: values)).
Proof.
  intros par_child hash_fn bytes program span field
    auxiliary semantic rest values accepted.
  pose proof accepted as source_reference.
  destruct (Recursive.checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [raw|] eqn:checked;
    [|unfold Recursive.recursive_auxiliary_reference in accepted;
       now rewrite checked in accepted].
  destruct (Recursive.checked_generated_raw_preserves_descriptor_body_and_validity
    bytes program (Source.SourceAuxiliary span field) raw checked)
    as [_ [source _]].
  assert (leaf : Source.raw_tree_of_source (S (length bytes)) bytes
      (Source.SourceAuxiliary span field) = Some (Node raw FNil)).
  { cbn [Source.raw_tree_of_source]. now rewrite source. }
  assert (raw_cut :
      raw = Recursive.recursive_auxiliary_raw auxiliary /\
      Recursive.recursive_auxiliary_semantic auxiliary = Some semantic).
  { unfold Recursive.recursive_auxiliary_reference in accepted.
    rewrite checked in accepted.
    cbv beta iota in accepted.
    rewrite Nat.eqb_refl in accepted.
    cbv beta iota in accepted.
    destruct (Schema.descriptor_payload field) eqn:payload;
      try discriminate.
    - destruct (Source.reduce_source_map_node bytes program
        (Schema.descriptor_slot field) par_child
        (Source.SourceAuxiliary span field)) as [entry|] eqn:reduced;
        [|discriminate].
      cbn [option_map] in accepted.
      destruct (Recursive.recursive_auxiliary_semantic
        (Recursive.RecursiveStringMapResult raw entry))
        as [derived|] eqn:cut; [|discriminate].
      cbn [option_map] in accepted.
      inversion accepted; subst auxiliary semantic.
      split; [reflexivity|exact cut].
    - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
        (Source.source_body bytes (Source.SourceAuxiliary span field)))
        as [[snapshot trie]|] eqn:physical; [|discriminate].
      destruct (Source.sequence_source_results
        (map par_child (EPM1.value_bodies snapshot)))
        as [converted|] eqn:ordered; [|discriminate].
      cbn [option_map] in accepted.
      destruct (Recursive.recursive_auxiliary_semantic
        (Recursive.RecursivePathMapResult raw snapshot trie converted))
        as [derived|] eqn:cut; [|discriminate].
      cbn [option_map] in accepted.
      inversion accepted; subst auxiliary semantic.
      split; [reflexivity|exact cut]. }
  destruct raw_cut as [same_raw same_cut].
  exists raw.
  repeat split; try assumption.
  unfold source_auxiliary_visit_transition.
  now rewrite leaf, source_reference.
Qed.

Definition source_auxiliary_visit_then_resume (steps : nat)
    (par_child : list nat -> option (bool * RPB3SemanticReducer.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (node : Source.source_tree) (rest : list Worklist.semantic_job)
    (values : list RPB3SemanticReducer.semantic_summary)
    : option (list RPB3SemanticReducer.semantic_summary) :=
  match source_auxiliary_visit_transition par_child hash_fn bytes
      program slot node rest values with
  | Some (_, (continued, delivered)) =>
      Worklist.run_semantic_frames steps continued delivered
  | None => None
  end.

Theorem accepted_source_auxiliary_visit_resumes_explicit_frames :
  forall steps par_child hash_fn bytes program span field
      auxiliary semantic rest values,
    Recursive.recursive_auxiliary_reference par_child hash_fn bytes
      program (Schema.descriptor_slot field)
      (Source.SourceAuxiliary span field) =
      Some (Recursive.RecursiveAuxiliaryResult auxiliary semantic) ->
    source_auxiliary_visit_then_resume steps par_child hash_fn bytes
      program (Schema.descriptor_slot field)
      (Source.SourceAuxiliary span field) rest values =
    Worklist.run_semantic_frames steps rest (semantic :: values).
Proof.
  intros steps par_child hash_fn bytes program span field
    auxiliary semantic rest values accepted.
  destruct (@accepted_recursive_auxiliary_source_visit_delivers_exact_cut
    par_child hash_fn bytes program span field auxiliary semantic
    rest values accepted) as [raw [_ [_ [_ [_ transition]]]]].
  unfold source_auxiliary_visit_then_resume.
  now rewrite transition.
Qed.

Print Assumptions decoded_source_auxiliary_references_keep_wire_order.
Print Assumptions auxiliary_field_reference_has_exact_source_leaf.
Print Assumptions accepted_recursive_root_semantics_keep_generated_source_order.
Print Assumptions accepted_recursive_root_has_exact_explicit_close_summary.
Print Assumptions accepted_recursive_auxiliary_source_visit_delivers_exact_cut.
Print Assumptions accepted_source_auxiliary_visit_resumes_explicit_frames.
End SourceAuxiliaryOrder.


Module SourceIndexedMutants.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Semantic := RPB3SemanticReducer.
Module Frames := SourceIndexedFrames.

Definition witness_hash (_ : list nat) : nat := 0.

Definition ordered_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 97; 34; 3; 10; 1; 98].

Definition reversed_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 98; 34; 3; 10; 1; 97].

Definition trailing_new_body : list nat := ordered_new_body ++ [0].

Definition private_unforgeable_body : list nat :=
  [10; 3; 10; 1; 42].

Definition truncated_private_unforgeable_body : list nat :=
  [10; 3; 10; 2; 42].

Definition mutant_scalar_visit_skips_checked_source
    (steps : nat)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (jobs : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result)
    : option (list Recursive.recursive_mixed_result) :=
  match steps, jobs with
  | S remaining,
      Frames.Visit program (Source.SourceScalar span field) :: rest =>
      match Source.raw_tree_of_source (S (length bytes)) bytes
          (Source.SourceScalar span field) with
      | Some (Node raw FNil) =>
          let semantic := Semantic.reduce_semantic_field raw [] in
          Frames.run_source_frames remaining par_child hash_fn bytes rest
            (Recursive.RecursiveScalarResult raw semantic :: values)
      | _ => None
      end
  | _, _ => Frames.run_source_frames steps par_child hash_fn bytes jobs values
  end.

Definition mutant_auxiliary_visit_uses_raw_leaf
    (steps : nat)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (jobs : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result)
    : option (list Recursive.recursive_mixed_result) :=
  match steps, jobs with
  | S remaining,
      Frames.Visit program (Source.SourceAuxiliary span field) :: rest =>
      match Source.raw_tree_of_source (S (length bytes)) bytes
          (Source.SourceAuxiliary span field) with
      | Some (Node raw FNil) =>
          let semantic := Semantic.reduce_semantic_field raw [] in
          if Semantic.sem_definitely_refused semantic then None
          else Frames.run_source_frames remaining par_child hash_fn bytes
            rest (Recursive.RecursiveScalarResult raw semantic :: values)
      | _ => None
      end
  | _, _ => Frames.run_source_frames steps par_child hash_fn bytes jobs values
  end.

Definition mutant_forest_swaps_first_two
    (steps : nat)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (jobs : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result)
    : option (list Recursive.recursive_mixed_result) :=
  match steps, jobs with
  | S remaining,
      Frames.VisitForest program (first :: second :: siblings) :: rest =>
      Frames.run_source_frames remaining par_child hash_fn bytes
        (Frames.Visit program second ::
         Frames.VisitForest program (first :: siblings) :: rest) values
  | _, _ => Frames.run_source_frames steps par_child hash_fn bytes jobs values
  end.

Definition mutant_close_omits_child_reverse
    (steps : nat)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (jobs : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result)
    : option (list Recursive.recursive_mixed_result) :=
  match steps, jobs with
  | S remaining, Frames.Close raw type_id arity :: rest =>
      if arity <=? length values then
        match Frames.close_message raw type_id (firstn arity values) with
        | Some result =>
            Frames.run_source_frames remaining par_child hash_fn bytes rest
              (result :: skipn arity values)
        | None => None
        end
      else None
  | _, _ => Frames.run_source_frames steps par_child hash_fn bytes jobs values
  end.

Definition default_connective_case
    : option (Schema.generated_program * Source.source_tree) :=
  match Schema.lookup_projected_program 1 [] [],
      Source.decode_root 1 [80; 0] with
  | Some program, Some [node] => Some (program, node)
  | _, _ => None
  end.

Definition ordered_new_auxiliary_case
    : option (Schema.generated_program * Source.source_tree) :=
  match Schema.lookup_projected_program 20 [] [],
      Source.decode_root 20 ordered_new_body with
  | Some program, Some (_ :: node :: _) => Some (program, node)
  | _, _ => None
  end.

Example source_indexed_ordered_generated_new_is_nonvacuous :
  exists summary,
    Frames.source_indexed_root_summary witness_hash 20 ordered_new_body =
      Some [summary] /\
    Frames.source_indexed_root_accepts witness_hash 20 ordered_new_body =
      true.
Proof. vm_compute. eexists. split; reflexivity. Qed.

Example source_indexed_reversed_generated_new_is_refused :
  Frames.source_indexed_root_summary witness_hash 20 reversed_new_body =
    None /\
  Frames.source_indexed_root_accepts witness_hash 20 reversed_new_body =
    false.
Proof. vm_compute. split; reflexivity. Qed.

Example source_indexed_nested_pathmap_is_nonvacuous :
  exists summary,
    Frames.source_indexed_root_summary witness_hash 1
      Recursive.nested_map_value_pathmap_par_wire = Some [summary].
Proof. vm_compute. eexists. reflexivity. Qed.

Example source_indexed_ordered_new_derives_exact_score :
  exists summary score,
    Frames.source_indexed_root_summary witness_hash 20 ordered_new_body =
      Some [summary] /\
    Semantic.sem_score_evidence summary = Semantic.ExactScore score /\
    Frames.source_indexed_root_accepts witness_hash 20 ordered_new_body =
      true.
Proof. vm_compute. eexists. eexists. repeat split; reflexivity. Qed.

Example source_indexed_ordered_new_accepts_exact_claimed_score :
  Frames.source_indexed_root_claimed_score witness_hash 20 ordered_new_body
    (Semantic.canonical_score_bytes
      Recursive.ordered_two_absent_value_maps_new_score) =
    Frames.ClaimedScoreAccepted.
Proof. vm_compute. reflexivity. Qed.

Example source_indexed_ordered_new_rejects_forged_claimed_score :
  Frames.source_indexed_root_claimed_score witness_hash 20 ordered_new_body
    (Semantic.canonical_score_bytes
      Recursive.ordered_two_absent_value_maps_new_score ++ [0]) =
    Frames.ClaimedScoreRejected.
Proof. vm_compute. reflexivity. Qed.

Example source_indexed_private_unforgeable_derives_exact_score :
  exists summary,
    Frames.source_indexed_root_summary witness_hash 60
      private_unforgeable_body = Some [summary] /\
    Semantic.sem_score_evidence summary =
      Semantic.ExactScore
        (Semantic.score_constructor 5%N [Semantic.ScoreBytes [42]]) /\
    Frames.source_indexed_root_accepts witness_hash 60
      private_unforgeable_body = true.
Proof. vm_compute. eexists. repeat split; reflexivity. Qed.

Example source_indexed_private_unforgeable_accepts_exact_claimed_score :
  Frames.source_indexed_root_claimed_score witness_hash 60
    private_unforgeable_body
    (Semantic.canonical_score_bytes
      (Semantic.score_constructor 5%N [Semantic.ScoreBytes [42]])) =
    Frames.ClaimedScoreAccepted.
Proof. vm_compute. reflexivity. Qed.

Example source_indexed_private_unforgeable_rejects_forged_claimed_score :
  Frames.source_indexed_root_claimed_score witness_hash 60
    private_unforgeable_body
    (Semantic.canonical_score_bytes
      (Semantic.score_constructor 5%N [Semantic.ScoreBytes [42]]) ++ [0]) =
    Frames.ClaimedScoreRejected.
Proof. vm_compute. reflexivity. Qed.

Example source_indexed_truncated_private_unforgeable_is_refused :
  Semantic.exact_unforgeable_leaf_bytes 61 [10; 2; 42] = None /\
  Frames.source_indexed_root_summary witness_hash 60
    truncated_private_unforgeable_body = None /\
  Frames.source_indexed_root_accepts witness_hash 60
    truncated_private_unforgeable_body = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example source_indexed_explicit_scalar_default_is_refused :
  Frames.source_indexed_root_summary witness_hash 1 [80; 0] = None /\
  Frames.source_indexed_root_accepts witness_hash 1 [80; 0] = false.
Proof. vm_compute. split; reflexivity. Qed.

Example source_indexed_trailing_root_byte_is_refused :
  Frames.source_indexed_root_summary witness_hash 20 trailing_new_body = None /\
  Frames.source_indexed_root_accepts witness_hash 20 trailing_new_body = false.
Proof. vm_compute. split; reflexivity. Qed.

Example mutant_scalar_visit_skips_checked_source_is_refuted :
  match default_connective_case with
  | Some (program, node) =>
      Frames.run_source_frames 1 (Recursive.recursive_par_reference witness_hash)
        witness_hash [80; 0] [Frames.Visit program node] [] = None /\
      exists result,
        mutant_scalar_visit_skips_checked_source 1
          (Recursive.recursive_par_reference witness_hash) witness_hash
          [80; 0] [Frames.Visit program node] [] = Some [result]
  | None => False
  end.
Proof. vm_compute. split; [reflexivity|eexists; reflexivity]. Qed.

Example mutant_auxiliary_visit_uses_raw_leaf_is_refuted :
  match ordered_new_auxiliary_case with
  | Some (program, node) =>
      (exists result,
        Frames.run_source_frames 1
          (Recursive.recursive_par_reference witness_hash) witness_hash
          ordered_new_body [Frames.Visit program node] [] = Some [result]) /\
      mutant_auxiliary_visit_uses_raw_leaf 1
        (Recursive.recursive_par_reference witness_hash) witness_hash
        ordered_new_body [Frames.Visit program node] [] = None
  | None => False
  end.
Proof. vm_compute. split; [eexists; reflexivity|reflexivity]. Qed.

Example mutant_forest_swaps_first_two_is_refuted :
  match Schema.lookup_projected_program 20 [] [],
      Source.decode_root 20 ordered_new_body with
  | Some program, Some nodes =>
      exists correct mutated,
        Frames.run_source_frames (Frames.source_forest_cost nodes)
          (Recursive.recursive_par_reference witness_hash) witness_hash
          ordered_new_body [Frames.VisitForest program nodes] [] =
            Some correct /\
        mutant_forest_swaps_first_two (Frames.source_forest_cost nodes)
          (Recursive.recursive_par_reference witness_hash) witness_hash
          ordered_new_body [Frames.VisitForest program nodes] [] =
            Some mutated /\
        correct <> mutated
  | _, _ => False
  end.
Proof.
  vm_compute.
  eexists. eexists.
  repeat split; try reflexivity.
  intro same. discriminate same.
Qed.

Example mutant_close_omits_child_reverse_is_refuted :
  match Recursive.recursive_mixed_root_reference witness_hash 20
      ordered_new_body with
  | Some results =>
      exists closed,
        Frames.run_source_frames 1
          (Recursive.recursive_par_reference witness_hash) witness_hash
          ordered_new_body
          [Frames.Close
            (SourceAuxiliaryOrder.source_generated_root_field 20
              ordered_new_body) 20 (length results)]
          (rev results) = Some [closed] /\
        mutant_close_omits_child_reverse 1
          (Recursive.recursive_par_reference witness_hash) witness_hash
          ordered_new_body
          [Frames.Close
            (SourceAuxiliaryOrder.source_generated_root_field 20
              ordered_new_body) 20 (length results)]
          (rev results) = None
  | None => False
  end.
Proof. vm_compute. eexists. split; reflexivity. Qed.

Print Assumptions source_indexed_ordered_generated_new_is_nonvacuous.
Print Assumptions source_indexed_reversed_generated_new_is_refused.
Print Assumptions source_indexed_nested_pathmap_is_nonvacuous.
Print Assumptions source_indexed_ordered_new_derives_exact_score.
Print Assumptions source_indexed_ordered_new_accepts_exact_claimed_score.
Print Assumptions source_indexed_ordered_new_rejects_forged_claimed_score.
Print Assumptions source_indexed_private_unforgeable_derives_exact_score.
Print Assumptions source_indexed_private_unforgeable_accepts_exact_claimed_score.
Print Assumptions source_indexed_private_unforgeable_rejects_forged_claimed_score.
Print Assumptions source_indexed_truncated_private_unforgeable_is_refused.
Print Assumptions source_indexed_explicit_scalar_default_is_refused.
Print Assumptions source_indexed_trailing_root_byte_is_refused.
Print Assumptions mutant_scalar_visit_skips_checked_source_is_refuted.
Print Assumptions mutant_auxiliary_visit_uses_raw_leaf_is_refuted.
Print Assumptions mutant_forest_swaps_first_two_is_refuted.
Print Assumptions mutant_close_omits_child_reverse_is_refuted.

End SourceIndexedMutants.
