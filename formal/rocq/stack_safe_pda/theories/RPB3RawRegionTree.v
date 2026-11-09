From Stdlib Require Import List Bool Arith Lia NArith.
From StackSafePDA Require Import StackSafePDA RPB3WireCursor RPB3GeneratedFieldRefinement.

Import ListNotations.

Module Wire := RPB3WireCursor.
Module Schema := RPB3GeneratedFieldRefinement.

Inductive source_tree :=
| SourceScalar (span : Wire.field_span) (field : Schema.field_descriptor)
| SourceMessage (span : Wire.field_span) (field : Schema.field_descriptor)
    (children : list source_tree)
| SourceAuxiliary (span : Wire.field_span) (field : Schema.field_descriptor).

Definition source_span (node : source_tree) : Wire.field_span :=
  match node with
  | SourceScalar span _ | SourceMessage span _ _ | SourceAuxiliary span _ => span
  end.

Definition source_field (node : source_tree) : Schema.field_descriptor :=
  match node with
  | SourceScalar _ field | SourceMessage _ field _ | SourceAuxiliary _ field => field
  end.

Definition source_body (bytes : list nat) (node : source_tree) : list nat :=
  let span := source_span node in
  firstn (Wire.span_end span - Wire.span_payload_start span)
    (skipn (Wire.span_payload_start span) bytes).

Lemma bounded_source_body_has_exact_length :
  forall bytes node,
    Wire.span_payload_start (source_span node) <=
      Wire.span_end (source_span node) ->
    Wire.span_end (source_span node) <= length bytes ->
    length (source_body bytes node) =
      Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node).
Proof.
  intros bytes node start_end end_bytes.
  unfold source_body.
  rewrite firstn_length, skipn_length.
  apply Nat.min_l. lia.
Qed.

Lemma source_body_byte_has_original_offset :
  forall bytes node index,
    index < Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node) ->
    nth_error (source_body bytes node) index =
      nth_error bytes (Wire.span_payload_start (source_span node) + index).
Proof.
  intros bytes node index within.
  unfold source_body.
  rewrite nth_error_firstn, nth_error_skipn.
  apply Nat.ltb_lt in within.
  now rewrite within.
Qed.

Lemma firstn_member_is_original :
  forall (A : Type) width (bytes : list A) value,
    In value (firstn width bytes) -> In value bytes.
Proof.
  intros A width.
  induction width as [|smaller IH];
    intros [|first rest] value member; simpl in *; try contradiction.
  destruct member as [same|later].
  - left. exact same.
  - right. now apply IH.
Qed.

Lemma skipn_member_is_original :
  forall (A : Type) offset (bytes : list A) value,
    In value (skipn offset bytes) -> In value bytes.
Proof.
  intros A offset.
  induction offset as [|smaller IH]; intros bytes value member.
  - exact member.
  - destruct bytes as [|first rest]; simpl in member;
      [contradiction|].
    right. now apply IH.
Qed.

Lemma source_body_inherits_global_byte_guard :
  forall bytes node,
    forallb Wire.byte_ok bytes = true ->
    forallb Wire.byte_ok (source_body bytes node) = true.
Proof.
  intros bytes node guarded.
  rewrite forallb_forall in guarded |- *.
  intros byte member.
  apply guarded.
  unfold source_body in member.
  eapply (skipn_member_is_original nat
    (Wire.span_payload_start (source_span node)) bytes byte).
  eapply (firstn_member_is_original nat
    (Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node))
    (skipn (Wire.span_payload_start (source_span node)) bytes) byte).
  exact member.
Qed.

Definition wire_kind_of_span (span : Wire.field_span) : Schema.wire_kind :=
  match Wire.span_wire span with
  | Wire.WireVarint => Schema.Varint
  | Wire.WireFixed64 => Schema.Fixed64
  | Wire.WireLength => Schema.LengthDelimited
  | Wire.WireFixed32 => Schema.Fixed32
  end.

Definition source_descriptor (program : Schema.generated_program)
    (span : Wire.field_span) : option Schema.field_descriptor :=
  match Schema.resolve_tag (Schema.program_fields program)
      (Wire.span_number span) with
  | Some field =>
      if Schema.wire_eqb (wire_kind_of_span span)
          (Schema.descriptor_wire field)
      then Some field else None
  | None => None
  end.

Lemma resolved_source_descriptor_has_actual_tag_and_wire :
  forall program span field,
    source_descriptor program span = Some field ->
    Schema.descriptor_tag field = Wire.span_number span /\
    Schema.wire_eqb (wire_kind_of_span span)
      (Schema.descriptor_wire field) = true.
Proof.
  intros program span field selected.
  unfold source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
    (Wire.span_number span)) as [candidate|] eqn:resolved;
    try discriminate.
  destruct (Schema.wire_eqb (wire_kind_of_span span)
    (Schema.descriptor_wire candidate)) eqn:matched;
    try discriminate.
  inversion selected; subst field.
  split; [|exact matched].
  clear selected matched.
  remember (Schema.program_fields program) as fields eqn:source_fields.
  clear source_fields.
  revert resolved.
  induction fields as [|head tail IH]; intro resolved;
    simpl in resolved; try discriminate.
  destruct (Schema.descriptor_tag head =? Wire.span_number span)
    eqn:same_tag.
  - inversion resolved; subst head.
    now apply Nat.eqb_eq in same_tag.
  - exact (IH resolved).
Qed.

Lemma successful_varint_advances :
  forall fuel bytes region_end offset group accumulator value final_offset,
    Wire.varint_groups fuel bytes region_end offset group accumulator =
      Some (value, final_offset) ->
    offset < final_offset.
Proof.
  induction fuel as [|fuel IH];
    intros bytes region_end offset group accumulator value final_offset parsed;
    cbn [Wire.varint_groups] in parsed; try discriminate.
  destruct (offset <? region_end); [|discriminate].
  destruct (nth_error bytes offset) as [byte|]; [|discriminate].
  destruct (Wire.byte_ok byte); [|discriminate].
  destruct ((group =? 9) && (1 <? byte)); [discriminate|].
  destruct (byte <? 128).
  - destruct ((0 <? group) && (byte =? 0)); [discriminate|].
    inversion parsed; lia.
  - apply IH in parsed. lia.
Qed.

Lemma successful_canonical_varint_advances :
  forall bytes region_end offset value final_offset,
    Wire.canonical_varint bytes region_end offset =
      Some (value, final_offset) ->
    offset < final_offset.
Proof.
  intros bytes region_end offset value final_offset parsed.
  unfold Wire.canonical_varint in parsed.
  eapply successful_varint_advances; eauto.
Qed.

Lemma successful_framing_has_strict_header :
  forall bytes region_end offset last ordered span,
    Wire.framing_field bytes region_end offset last ordered = Some span ->
    offset < Wire.span_payload_start span.
Proof.
  intros bytes region_end offset last ordered span parsed.
  unfold Wire.framing_field in parsed.
  destruct (Wire.canonical_varint bytes region_end offset)
    as [[key key_end]|] eqn:key_read; [|discriminate].
  pose proof (successful_canonical_varint_advances _ _ _ _ _ key_read)
    as key_progress.
  destruct (((0 <? (key / 8))%N) &&
    Wire.field_number_within_limit (key / 8)%N); [|discriminate].
  destruct (negb ordered || (last <=? N.to_nat (key / 8)%N));
    [|discriminate].
  destruct (N.to_nat (key mod 8)%N)
    as [|[|[|[|[|[|code]]]]]]; try discriminate.
  - destruct (Wire.canonical_varint bytes region_end key_end)
      as [[value value_end]|]; [|discriminate].
    inversion parsed; simpl; lia.
  - destruct (key_end + 8 <=? region_end); [|discriminate].
    inversion parsed; simpl; lia.
  - destruct (Wire.canonical_varint bytes region_end key_end)
      as [[payload_length body_start]|] eqn:length; [|discriminate].
    pose proof (successful_canonical_varint_advances _ _ _ _ _ length)
      as length_progress.
    destruct (N.leb payload_length
      (N.of_nat (region_end - body_start))); [|discriminate].
    inversion parsed; simpl; lia.
  - destruct (key_end + 4 <=? region_end); [|discriminate].
    inversion parsed; simpl; lia.
Qed.

Lemma successful_checked_field_has_strict_header :
  forall bytes region_end offset last ordered span,
    Wire.checked_field bytes region_end offset last ordered = Some span ->
    offset < Wire.span_payload_start span.
Proof.
  intros bytes region_end offset last ordered span parsed.
  unfold Wire.checked_field in parsed.
  destruct (Wire.framing_field bytes region_end offset last ordered)
    as [field|] eqn:framed; [|discriminate].
  destruct ((Wire.span_start field =? offset) &&
    (offset <? Wire.span_end field) &&
    (Wire.span_end field <=? region_end)); [|discriminate].
  inversion parsed; subst.
  eapply successful_framing_has_strict_header; eauto.
Qed.

Lemma successful_framing_payload_stays_inside_span :
  forall bytes region_end offset last ordered span,
    Wire.framing_field bytes region_end offset last ordered = Some span ->
    Wire.span_payload_start span <= Wire.span_end span.
Proof.
  intros bytes region_end offset last ordered span parsed.
  unfold Wire.framing_field in parsed.
  destruct (Wire.canonical_varint bytes region_end offset)
    as [[key key_end]|]; [|discriminate].
  destruct (((0 <? (key / 8))%N) &&
    Wire.field_number_within_limit (key / 8)%N); [|discriminate].
  destruct (negb ordered || (last <=? N.to_nat (key / 8)%N));
    [|discriminate].
  destruct (N.to_nat (key mod 8)%N)
    as [|[|[|[|[|[|code]]]]]]; try discriminate.
  - destruct (Wire.canonical_varint bytes region_end key_end)
      as [[value value_end]|] eqn:value_read; [|discriminate].
    pose proof (successful_canonical_varint_advances
      _ _ _ _ _ value_read) as value_progress.
    inversion parsed; simpl; lia.
  - destruct (key_end + 8 <=? region_end); [|discriminate].
    inversion parsed; simpl; lia.
  - destruct (Wire.canonical_varint bytes region_end key_end)
      as [[payload_length body_start]|]; [|discriminate].
    destruct (N.leb payload_length
      (N.of_nat (region_end - body_start))); [|discriminate].
    inversion parsed; simpl; lia.
  - destruct (key_end + 4 <=? region_end); [|discriminate].
    inversion parsed; simpl; lia.
Qed.

Lemma successful_checked_field_payload_stays_inside_span :
  forall bytes region_end offset last ordered span,
    Wire.checked_field bytes region_end offset last ordered = Some span ->
    Wire.span_payload_start span <= Wire.span_end span.
Proof.
  intros bytes region_end offset last ordered span parsed.
  unfold Wire.checked_field in parsed.
  destruct (Wire.framing_field bytes region_end offset last ordered)
    as [field|] eqn:framed; [|discriminate].
  destruct ((Wire.span_start field =? offset) &&
    (offset <? Wire.span_end field) &&
    (Wire.span_end field <=? region_end)); [|discriminate].
  inversion parsed; subst.
  eapply successful_framing_payload_stays_inside_span; eauto.
Qed.

Lemma source_forall_member :
  forall (A : Type) (predicate : A -> Prop) members member,
    Forall predicate members -> In member members -> predicate member.
Proof.
  intros A predicate members member all included.
  induction all as [|head tail head_valid tail_valid IH].
  - inversion included.
  - destruct included as [same|later].
    + subst; exact head_valid.
    + exact (IH later).
Qed.

Lemma reference_spans_stay_inside_their_region :
  forall fuel bytes region_end offset last ordered spans,
    Wire.reference_remaining fuel bytes region_end offset last ordered =
      Wire.Parsed spans ->
    Forall (fun span =>
      offset <= Wire.span_start span /\
      offset < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans.
Proof.
  induction fuel as [|fuel IH];
    intros bytes region_end offset last ordered spans parsed;
    cbn [Wire.reference_remaining] in parsed.
  - destruct (offset =? region_end) eqn:eof.
    + inversion parsed; constructor.
    + destruct (Wire.checked_field bytes region_end offset last ordered);
        discriminate.
  - destruct (offset =? region_end) eqn:eof.
    + inversion parsed; constructor.
    + destruct (Wire.checked_field bytes region_end offset last ordered)
        as [field|] eqn:checked; [|discriminate].
      destruct (Wire.reference_remaining fuel bytes region_end
        (Wire.span_end field) (Wire.span_number field) ordered)
        as [suffix| |] eqn:rest; try discriminate.
      inversion parsed; subst spans.
      pose proof (Wire.checked_field_has_strict_bounded_progress
        _ _ _ _ _ _ checked) as [field_start [progress bounded]].
      pose proof (successful_checked_field_has_strict_header
        _ _ _ _ _ _ checked) as header.
      pose proof (successful_checked_field_payload_stays_inside_span
        _ _ _ _ _ _ checked) as payload_bounded.
      constructor.
      * simpl. repeat split; lia.
      * apply Forall_forall. intros child in_suffix.
        pose proof (IH _ _ _ _ _ _ rest) as suffix_bounds.
        pose proof (source_forall_member _ _ _ _
          suffix_bounds in_suffix) as child_bounds.
        destruct child_bounds as
          [child_start [child_header [child_payload child_end]]].
        simpl in *. repeat split; lia.
Qed.

Lemma parsed_region_spans_stay_inside_their_region :
  forall bytes region_start region_end ordered spans,
    Wire.worklist_region bytes region_start region_end ordered =
      Wire.Parsed spans ->
    Forall (fun span =>
      region_start <= Wire.span_start span /\
      region_start < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans.
Proof.
  intros bytes region_start region_end ordered spans parsed.
  rewrite Wire.worklist_and_reference_agree_on_every_finite_region in parsed.
  unfold Wire.reference_region in parsed.
  destruct (Wire.region_ok bytes region_start region_end);
    [|discriminate].
  eapply reference_spans_stay_inside_their_region; eauto.
Qed.

Theorem parsed_generated_message_child_has_strictly_smaller_region :
  forall bytes region_start region_end program spans span field,
    Wire.worklist_region bytes region_start region_end false =
      Wire.Parsed spans ->
    In span spans ->
    source_descriptor program span = Some field ->
    Schema.descriptor_payload field = Schema.Message ->
    Wire.span_end span - Wire.span_payload_start span <
      region_end - region_start.
Proof.
  intros bytes region_start region_end program spans span field
    parsed member resolved message.
  pose proof (parsed_region_spans_stay_inside_their_region
    _ _ _ _ _ parsed) as bounded.
  pose proof (source_forall_member _ _ _ _ bounded member)
    as span_bounds.
  destruct span_bounds as [within [header [payload end_within]]].
  lia.
Qed.

Inductive parsed_generated_message_path (bytes : list nat)
    : Schema.generated_program -> nat -> nat -> nat -> Prop :=
| ParsedPathEnd : forall program region_start region_end,
    parsed_generated_message_path bytes program region_start region_end 0
| ParsedPathChild :
    forall program region_start region_end spans span field type_id
      child_program depth,
      Wire.worklist_region bytes region_start region_end false =
        Wire.Parsed spans ->
      In span spans ->
      source_descriptor program span = Some field ->
      Schema.descriptor_payload field = Schema.Message ->
      Schema.descriptor_child_type_id field = Some type_id ->
      Schema.lookup_projected_program type_id [] [] = Some child_program ->
      parsed_generated_message_path bytes child_program
        (Wire.span_payload_start span) (Wire.span_end span) depth ->
      parsed_generated_message_path bytes program region_start region_end
        (S depth).

Theorem parsed_generated_message_depth_fits_region :
  forall bytes program region_start region_end depth,
    parsed_generated_message_path bytes program
      region_start region_end depth ->
    depth <= region_end - region_start.
Proof.
  intros bytes program region_start region_end depth path.
  induction path as
    [program start finish
    |program start finish spans span field type_id child_program depth
      parsed member selected message child_id lookup child_path IH].
  - lia.
  - pose proof (parsed_generated_message_child_has_strictly_smaller_region
      _ _ _ _ _ _ _ parsed member selected message) as strict.
    lia.
Qed.

Theorem root_generated_message_path_cannot_exhaust_byte_fuel :
  forall bytes program depth,
    parsed_generated_message_path bytes program 0 (length bytes) depth ->
    0 < S (length bytes) - depth.
Proof.
  intros bytes program depth path.
  pose proof (parsed_generated_message_depth_fits_region
    _ _ _ _ _ path) as bounded.
  lia.
Qed.

Record decode_accumulator := DecodeAccumulator {
  decode_last_slot : option nat;
  decode_seen_slots : list nat;
  decode_reversed : list source_tree
}.

Definition slot_admissible (program : Schema.generated_program)
    (state : decode_accumulator) (field : Schema.field_descriptor) : bool :=
  let slot := Schema.descriptor_slot field in
  (slot <? Schema.program_slot_count program) &&
  Schema.slot_follows (decode_last_slot state) slot
    (Schema.descriptor_repeated field) &&
  (Schema.descriptor_repeated field ||
    negb (existsb (Nat.eqb slot) (decode_seen_slots state))).

Lemma admitted_source_slot_obeys_generated_order :
  forall program state field,
    slot_admissible program state field = true ->
    Schema.descriptor_slot field < Schema.program_slot_count program /\
    Schema.slot_follows (decode_last_slot state)
      (Schema.descriptor_slot field) (Schema.descriptor_repeated field) = true /\
    (Schema.descriptor_repeated field ||
      negb (existsb (Nat.eqb (Schema.descriptor_slot field))
        (decode_seen_slots state))) = true.
Proof.
  intros program state field admitted.
  unfold slot_admissible in admitted.
  repeat rewrite andb_true_iff in admitted.
  destruct admitted as [[within ordered] unique].
  apply Nat.ltb_lt in within.
  now repeat split.
Qed.

Fixpoint decode_region (fuel : nat) (bytes : list nat)
    (program : Schema.generated_program) (region_start region_end : nat)
    {struct fuel} : option (list source_tree) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match Wire.worklist_region bytes region_start region_end false with
      | Wire.Rejected | Wire.BudgetExhausted => None
      | Wire.Parsed spans =>
          let step := fun (state : option decode_accumulator)
              (span : Wire.field_span) =>
            match state, source_descriptor program span with
            | Some current, Some field =>
                if slot_admissible program current field then
                  let node :=
                    match Schema.descriptor_payload field with
                    | Schema.Scalar => Some (SourceScalar span field)
                    | Schema.Message =>
                        match Schema.descriptor_child_type_id field with
                        | Some type_id =>
                            match Schema.lookup_projected_program type_id [] [] with
                            | Some child_program =>
                                option_map (SourceMessage span field)
                                  (decode_region smaller bytes child_program
                                    (Wire.span_payload_start span)
                                    (Wire.span_end span))
                            | None => None
                            end
                        | None => None
                        end
                    | Schema.StringMap | Schema.PathMap =>
                        Some (SourceAuxiliary span field)
                    end in
                  option_map (fun selected =>
                    DecodeAccumulator (Some (Schema.descriptor_slot field))
                      (Schema.descriptor_slot field :: decode_seen_slots current)
                      (selected :: decode_reversed current)) node
                else None
            | _, _ => None
            end in
          option_map (fun state => rev (decode_reversed state))
            (fold_left step spans (Some (DecodeAccumulator None [] [])))
      end
  end.

Definition generated_source_node
    (decode_child : Schema.generated_program -> nat -> nat ->
      option (list source_tree))
    (span : Wire.field_span) (field : Schema.field_descriptor)
    : option source_tree :=
  match Schema.descriptor_payload field with
  | Schema.Scalar => Some (SourceScalar span field)
  | Schema.Message =>
      match Schema.descriptor_child_type_id field with
      | Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | Some child_program =>
              option_map (SourceMessage span field)
                (decode_child child_program
                  (Wire.span_payload_start span) (Wire.span_end span))
          | None => None
          end
      | None => None
      end
  | Schema.StringMap | Schema.PathMap =>
      Some (SourceAuxiliary span field)
  end.

Definition decode_source_step
    (program : Schema.generated_program)
    (decode_child : Schema.generated_program -> nat -> nat ->
      option (list source_tree))
    (state : option decode_accumulator) (span : Wire.field_span)
    : option decode_accumulator :=
  match state, source_descriptor program span with
  | Some current, Some field =>
      if slot_admissible program current field then
        option_map (fun selected =>
          DecodeAccumulator (Some (Schema.descriptor_slot field))
            (Schema.descriptor_slot field :: decode_seen_slots current)
            (selected :: decode_reversed current))
          (generated_source_node decode_child span field)
      else None
  | _, _ => None
  end.

Definition source_node_matches_generated_descriptor
    (program : Schema.generated_program) (node : source_tree) : Prop :=
  source_descriptor program (source_span node) =
    Some (source_field node) /\
  (Schema.descriptor_payload (source_field node) = Schema.StringMap ->
    node = SourceAuxiliary (source_span node) (source_field node)).

Lemma generated_source_node_has_selected_constructor :
  forall decode_child span field node,
    generated_source_node decode_child span field = Some node ->
    source_span node = span /\ source_field node = field /\
    (Schema.descriptor_payload field = Schema.StringMap ->
      node = SourceAuxiliary span field).
Proof.
  intros decode_child span field node generated.
  unfold generated_source_node in generated.
  destruct (Schema.descriptor_payload field) eqn:payload;
    simpl in generated.
  - inversion generated; subst node; simpl.
    repeat split; try reflexivity.
    intro impossible. discriminate.
  - destruct (Schema.descriptor_child_type_id field)
      as [type_id|]; [|discriminate].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|]; [|discriminate].
    destruct (decode_child child_program
      (Wire.span_payload_start span) (Wire.span_end span))
      as [children|]; [|discriminate].
    inversion generated; subst node; simpl.
    repeat split; try reflexivity.
    intro impossible. discriminate.
  - inversion generated; subst node; simpl.
    now repeat split.
  - inversion generated; subst node; simpl.
    repeat split; try reflexivity.
Qed.

Lemma decode_source_step_preserves_generated_constructor :
  forall program decode_child current span next,
    decode_source_step program decode_child (Some current) span =
      Some next ->
    Forall (source_node_matches_generated_descriptor program)
      (decode_reversed current) ->
    Forall (source_node_matches_generated_descriptor program)
      (decode_reversed next).
Proof.
  intros program decode_child current span next stepped previous.
  unfold decode_source_step in stepped.
  destruct (source_descriptor program span) as [field|]
    eqn:resolved; [|discriminate].
  destruct (slot_admissible program current field);
    [|discriminate].
  destruct (generated_source_node decode_child span field)
    as [node|] eqn:generated; [|discriminate].
  inversion stepped; subst next; simpl.
  constructor; [|exact previous].
  destruct (generated_source_node_has_selected_constructor
    decode_child span field node generated)
    as [source_span_eq [source_field_eq auxiliary_shape]].
  unfold source_node_matches_generated_descriptor.
  rewrite source_span_eq, source_field_eq, resolved.
  split; [reflexivity|exact auxiliary_shape].
Qed.

Lemma fold_left_none_stays_none :
  forall (A B : Type) (step : option A -> B -> option A) members,
    (forall member, step None member = None) ->
    fold_left step members None = None.
Proof.
  intros A B step members absorbing.
  induction members as [|member tail IH]; simpl; [reflexivity|].
  now rewrite absorbing.
Qed.

Lemma successful_option_fold_preserves_invariant :
  forall (A B : Type) (step : option A -> B -> option A)
      (invariant : A -> Prop),
    (forall member, step None member = None) ->
    (forall current member next,
      step (Some current) member = Some next ->
      invariant current -> invariant next) ->
    forall members initial final,
      invariant initial ->
      fold_left step members (Some initial) = Some final ->
      invariant final.
Proof.
  intros A B step invariant absorbing preserves members.
  induction members as [|member tail IH];
    intros initial final initial_valid folded; simpl in folded.
  - now inversion folded; subst.
  - destruct (step (Some initial) member) as [next|]
      eqn:advanced.
    + apply (IH next final).
      * eapply preserves; eauto.
      * exact folded.
    + rewrite (fold_left_none_stays_none A B step tail absorbing)
        in folded.
      discriminate.
Qed.

Lemma source_forall_reverses :
  forall (A : Type) (predicate : A -> Prop) members,
    Forall predicate members -> Forall predicate (rev members).
Proof.
  intros A predicate members included.
  induction included as [|member tail present included IH];
    simpl; [constructor|].
  apply Forall_app.
  split; [exact IH|].
  constructor; [exact present|constructor].
Qed.

Theorem decoded_region_nodes_match_generated_source :
  forall fuel bytes program region_start region_end nodes,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    Forall (source_node_matches_generated_descriptor program) nodes.
Proof.
  intros [|smaller] bytes program region_start region_end nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate.
  set (decode_child := fun (child_program : Schema.generated_program)
    (start finish : nat) =>
      decode_region smaller bytes child_program start finish).
  change (option_map (fun state => rev (decode_reversed state))
    (fold_left (decode_source_step program decode_child) spans
      (Some (DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (decode_source_step program decode_child) spans
    (Some (DecodeAccumulator None [] []))) as [final|]
    eqn:folded; [|discriminate].
  inversion decoded; subst nodes.
  apply source_forall_reverses.
  eapply successful_option_fold_preserves_invariant with
    (step := decode_source_step program decode_child)
    (invariant := fun current =>
      Forall (source_node_matches_generated_descriptor program)
        (decode_reversed current))
    (initial := DecodeAccumulator None [] []);
    [| |constructor|exact folded].
  - intro member. reflexivity.
  - intros current member next advanced previous.
    eapply decode_source_step_preserves_generated_constructor;
      [exact advanced|exact previous].
Qed.

Definition source_node_inside_region (region_start region_end : nat)
    (node : source_tree) : Prop :=
  region_start < Wire.span_payload_start (source_span node) /\
  Wire.span_payload_start (source_span node) <=
    Wire.span_end (source_span node) /\
  Wire.span_end (source_span node) <= region_end.

Lemma decoded_source_step_preserves_region_bounds :
  forall program decode_child current span next region_start region_end,
    decode_source_step program decode_child (Some current) span =
      Some next ->
    region_start < Wire.span_payload_start span /\
    Wire.span_payload_start span <= Wire.span_end span /\
    Wire.span_end span <= region_end ->
    Forall (source_node_inside_region region_start region_end)
      (decode_reversed current) ->
    Forall (source_node_inside_region region_start region_end)
      (decode_reversed next).
Proof.
  intros program decode_child current span next region_start region_end
    advanced bounded previous.
  unfold decode_source_step in advanced.
  destruct (source_descriptor program span) as [field|]
    eqn:selected; [|discriminate].
  destruct (slot_admissible program current field);
    [|discriminate].
  destruct (generated_source_node decode_child span field)
    as [node|] eqn:generated; [|discriminate].
  inversion advanced; subst next; clear advanced.
  destruct (generated_source_node_has_selected_constructor
    decode_child span field node generated) as [same_span _].
  constructor; [|exact previous].
  unfold source_node_inside_region in *.
  now rewrite same_span.
Qed.

Lemma successful_source_fold_preserves_region_bounds :
  forall program decode_child spans current final region_start region_end,
    Forall (fun span =>
      region_start < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans ->
    Forall (source_node_inside_region region_start region_end)
      (decode_reversed current) ->
    fold_left (decode_source_step program decode_child) spans
      (Some current) = Some final ->
    Forall (source_node_inside_region region_start region_end)
      (decode_reversed final).
Proof.
  intros program decode_child spans.
  induction spans as [|span rest IH];
    intros current final region_start region_end span_bounds previous folded.
  - simpl in folded. now inversion folded; subst final.
  - inversion span_bounds as [|head tail bounded suffix]; subst.
    cbn [fold_left] in folded.
    destruct (decode_source_step program decode_child
      (Some current) span) as [next|] eqn:advanced.
    + eapply IH; [exact suffix| |exact folded].
      eapply decoded_source_step_preserves_region_bounds;
        [exact advanced|exact bounded|exact previous].
    + assert (absorbed :
        fold_left (decode_source_step program decode_child)
          rest None = None).
      { apply fold_left_none_stays_none.
        intro member. reflexivity. }
      rewrite absorbed in folded. discriminate.
Qed.

Theorem decoded_region_nodes_have_checked_source_windows :
  forall fuel bytes program region_start region_end nodes,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    region_end <= length bytes /\
    Forall (source_node_inside_region region_start region_end) nodes.
Proof.
  intros [|smaller] bytes program region_start region_end nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate.
  assert (end_within : region_end <= length bytes).
  { unfold Wire.worklist_region in framed.
    destruct (Wire.region_ok bytes region_start region_end)
      eqn:guard; [|discriminate].
    unfold Wire.region_ok in guard.
    apply andb_true_iff in guard as [bounds _].
    apply andb_true_iff in bounds as [_ end_guard].
    now apply Nat.leb_le in end_guard. }
  assert (span_bounds : Forall (fun span =>
      region_start < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans).
  { pose proof (parsed_region_spans_stay_inside_their_region
      bytes region_start region_end false spans framed) as parsed.
    apply Forall_forall.
    intros span member.
    destruct (source_forall_member _ _ _ _ parsed member)
      as [_ [header [payload bounded]]].
    now repeat split. }
  set (decode_child := fun (child_program : Schema.generated_program)
    (start finish : nat) =>
    decode_region smaller bytes child_program start finish).
  change (option_map (fun state => rev (decode_reversed state))
    (fold_left (decode_source_step program decode_child) spans
      (Some (DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (decode_source_step program decode_child) spans
    (Some (DecodeAccumulator None [] []))) as [final|]
    eqn:folded; [|discriminate].
  inversion decoded; subst nodes.
  split; [exact end_within|].
  apply source_forall_reverses.
  eapply (successful_source_fold_preserves_region_bounds
    program decode_child spans (DecodeAccumulator None [] []) final
    region_start region_end);
    [exact span_bounds|constructor|exact folded].
Qed.

Inductive recursive_source_window : nat -> nat -> source_tree -> Prop :=
| RecursiveSourceScalar : forall region_start region_end span field,
    source_node_inside_region region_start region_end
      (SourceScalar span field) ->
    recursive_source_window region_start region_end
      (SourceScalar span field)
| RecursiveSourceMessage : forall region_start region_end span field children,
    source_node_inside_region region_start region_end
      (SourceMessage span field children) ->
    Forall (recursive_source_window
      (Wire.span_payload_start span) (Wire.span_end span)) children ->
    recursive_source_window region_start region_end
      (SourceMessage span field children)
| RecursiveSourceAuxiliary : forall region_start region_end span field,
    source_node_inside_region region_start region_end
      (SourceAuxiliary span field) ->
    recursive_source_window region_start region_end
      (SourceAuxiliary span field).

Lemma generated_source_node_has_recursive_window :
  forall decode_child span field node region_start region_end,
    (forall child_program child_start child_end children,
      decode_child child_program child_start child_end = Some children ->
      Forall (recursive_source_window child_start child_end) children) ->
    generated_source_node decode_child span field = Some node ->
    region_start < Wire.span_payload_start span /\
    Wire.span_payload_start span <= Wire.span_end span /\
    Wire.span_end span <= region_end ->
    recursive_source_window region_start region_end node.
Proof.
  intros decode_child span field node region_start region_end
    children_sound produced bounded.
  unfold generated_source_node in produced.
  destruct (Schema.descriptor_payload field) eqn:payload.
  - inversion produced; subst node.
    apply RecursiveSourceScalar.
    unfold source_node_inside_region; simpl. exact bounded.
  - destruct (Schema.descriptor_child_type_id field)
      as [type_id|] eqn:child; [|discriminate].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|] eqn:lookup; [|discriminate].
    destruct (decode_child child_program
      (Wire.span_payload_start span) (Wire.span_end span))
      as [children|] eqn:decoded; [|discriminate].
    inversion produced; subst node.
    apply RecursiveSourceMessage.
    + unfold source_node_inside_region; simpl. exact bounded.
    + eapply children_sound; exact decoded.
  - inversion produced; subst node.
    apply RecursiveSourceAuxiliary.
    unfold source_node_inside_region; simpl. exact bounded.
  - inversion produced; subst node.
    apply RecursiveSourceAuxiliary.
    unfold source_node_inside_region; simpl. exact bounded.
Qed.

Lemma decoded_source_step_preserves_recursive_windows :
  forall program decode_child current span next region_start region_end,
    (forall child_program child_start child_end children,
      decode_child child_program child_start child_end = Some children ->
      Forall (recursive_source_window child_start child_end) children) ->
    decode_source_step program decode_child (Some current) span =
      Some next ->
    region_start < Wire.span_payload_start span /\
    Wire.span_payload_start span <= Wire.span_end span /\
    Wire.span_end span <= region_end ->
    Forall (recursive_source_window region_start region_end)
      (decode_reversed current) ->
    Forall (recursive_source_window region_start region_end)
      (decode_reversed next).
Proof.
  intros program decode_child current span next region_start region_end
    children_sound advanced bounded previous.
  unfold decode_source_step in advanced.
  destruct (source_descriptor program span) as [field|]
    eqn:selected; [|discriminate].
  destruct (slot_admissible program current field); [|discriminate].
  destruct (generated_source_node decode_child span field)
    as [node|] eqn:produced; [|discriminate].
  inversion advanced; subst next; clear advanced.
  constructor; [|exact previous].
  eapply generated_source_node_has_recursive_window;
    [exact children_sound|exact produced|exact bounded].
Qed.

Lemma successful_source_fold_preserves_recursive_windows :
  forall program decode_child spans current final region_start region_end,
    (forall child_program child_start child_end children,
      decode_child child_program child_start child_end = Some children ->
      Forall (recursive_source_window child_start child_end) children) ->
    Forall (fun span =>
      region_start < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans ->
    Forall (recursive_source_window region_start region_end)
      (decode_reversed current) ->
    fold_left (decode_source_step program decode_child) spans
      (Some current) = Some final ->
    Forall (recursive_source_window region_start region_end)
      (decode_reversed final).
Proof.
  intros program decode_child spans.
  induction spans as [|span rest IH]; intros current final
    region_start region_end children_sound span_bounds previous folded.
  - simpl in folded. now inversion folded; subst final.
  - inversion span_bounds as [|head tail bounded suffix]; subst.
    cbn [fold_left] in folded.
    destruct (decode_source_step program decode_child
      (Some current) span) as [next|] eqn:advanced.
    + eapply IH; [exact children_sound|exact suffix| |exact folded].
      eapply decoded_source_step_preserves_recursive_windows;
        [exact children_sound|exact advanced|exact bounded|exact previous].
    + rewrite (fold_left_none_stays_none _ _
        (decode_source_step program decode_child) rest) in folded.
      * discriminate.
      * intro member. reflexivity.
Qed.

Theorem decoded_region_nodes_have_recursive_source_windows :
  forall fuel bytes program region_start region_end nodes,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    Forall (recursive_source_window region_start region_end) nodes.
Proof.
  induction fuel as [|smaller IH]; intros bytes program
    region_start region_end nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate.
  assert (span_bounds : Forall (fun span =>
      region_start < Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= region_end) spans).
  { pose proof (parsed_region_spans_stay_inside_their_region
      bytes region_start region_end false spans framed) as parsed.
    apply Forall_forall.
    intros span member.
    destruct (source_forall_member _ _ _ _ parsed member)
      as [_ [header [payload bounded]]].
    now repeat split. }
  set (decode_child := fun (child_program : Schema.generated_program)
    (start finish : nat) =>
    decode_region smaller bytes child_program start finish).
  assert (children_sound : forall child_program child_start child_end children,
      decode_child child_program child_start child_end = Some children ->
      Forall (recursive_source_window child_start child_end) children).
  { intros child_program child_start child_end children child_decoded.
    unfold decode_child in child_decoded.
    eapply IH; exact child_decoded. }
  change (option_map (fun state => rev (decode_reversed state))
    (fold_left (decode_source_step program decode_child) spans
      (Some (DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (decode_source_step program decode_child) spans
    (Some (DecodeAccumulator None [] []))) as [final|]
    eqn:folded; [|discriminate].
  inversion decoded; subst nodes.
  apply source_forall_reverses.
  eapply (successful_source_fold_preserves_recursive_windows
    program decode_child spans (DecodeAccumulator None [] []) final
    region_start region_end);
    [exact children_sound|exact span_bounds|constructor|exact folded].
Qed.

Inductive source_tree_descendant : source_tree -> source_tree -> Prop :=
| SourceDescendantHere : forall node,
    source_tree_descendant node node
| SourceDescendantMessageChild : forall span field children child descendant,
    In child children ->
    source_tree_descendant child descendant ->
    source_tree_descendant (SourceMessage span field children) descendant.

Lemma recursive_source_window_has_bounded_node :
  forall region_start region_end node,
    recursive_source_window region_start region_end node ->
    source_node_inside_region region_start region_end node.
Proof.
  intros region_start region_end node window.
  now inversion window.
Qed.

Lemma recursive_source_window_reaches_descendant :
  forall region_start region_end node descendant,
    recursive_source_window region_start region_end node ->
    source_tree_descendant node descendant ->
    exists descendant_start descendant_end,
      recursive_source_window descendant_start descendant_end descendant /\
      descendant_end <= region_end.
Proof.
  intros region_start region_end node descendant window reached.
  revert region_start region_end window.
  induction reached as [node|span field children child descendant
      member reached IH]; intros region_start region_end window.
  - exists region_start, region_end. now split.
  - inversion window as [|? ? ? ? ? bounded children_windows|]; subst.
    rewrite Forall_forall in children_windows.
    destruct (IH (Wire.span_payload_start span) (Wire.span_end span)
      (children_windows child member)) as
      [descendant_start [descendant_end [descendant_window inside_child]]].
    exists descendant_start, descendant_end.
    split; [exact descendant_window|].
    unfold source_node_inside_region in bounded; simpl in bounded.
    lia.
Qed.

Theorem decoded_source_node_has_exact_bounded_body :
  forall fuel bytes program region_start region_end nodes node,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    In node nodes ->
    length (source_body bytes node) =
      Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node).
Proof.
  intros fuel bytes program region_start region_end nodes node
    decoded member.
  destruct (decoded_region_nodes_have_checked_source_windows
    fuel bytes program region_start region_end nodes decoded)
    as [end_within bounded].
  pose proof (source_forall_member _ _ _ _ bounded member)
    as [_ [payload_end end_region]].
  apply bounded_source_body_has_exact_length; lia.
Qed.

Theorem decoded_root_checks_every_source_byte :
  forall fuel bytes program nodes,
    decode_region fuel bytes program 0 (length bytes) = Some nodes ->
    forallb Wire.byte_ok bytes = true.
Proof.
  intros [|smaller] bytes program nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Wire.worklist_region bytes 0 (length bytes) false)
    eqn:framed; try discriminate.
  unfold Wire.worklist_region in framed.
  destruct (Wire.region_ok bytes 0 (length bytes))
    eqn:guard; [|discriminate].
  unfold Wire.region_ok in guard.
  simpl in guard.
  rewrite Nat.sub_0_r, firstn_all in guard.
  repeat rewrite andb_true_iff in guard.
  tauto.
Qed.

Lemma fold_left_agrees_when_steps_agree_on_members :
  forall (A B : Type) (left right : A -> B -> A) members initial,
    (forall member, In member members ->
      forall state, left state member = right state member) ->
    fold_left left members initial = fold_left right members initial.
Proof.
  intros A B left right members.
  induction members as [|head tail IH]; intros initial same; simpl.
  - reflexivity.
  - rewrite (same head (or_introl eq_refl) initial).
    apply IH. intros member in_tail state.
    apply same; right; exact in_tail.
Qed.

Theorem decode_region_is_stable_above_its_byte_depth :
  forall fuel bytes program region_start region_end,
    region_end - region_start < fuel ->
    decode_region (S fuel) bytes program region_start region_end =
      decode_region fuel bytes program region_start region_end.
Proof.
  induction fuel as [|fuel IH];
    intros bytes program region_start region_end enough.
  - lia.
  - unfold decode_region at 1 2.
    cbn beta iota zeta.
    destruct (Wire.worklist_region bytes region_start region_end false)
      as [spans| |] eqn:parsed; try reflexivity.
    f_equal.
    apply fold_left_agrees_when_steps_agree_on_members.
    intros span member state.
    destruct state as [current|]; [|reflexivity].
    destruct (source_descriptor program span) as [field|]
      eqn:selected; [|reflexivity].
    destruct (slot_admissible program current field); [|reflexivity].
    destruct (Schema.descriptor_payload field) eqn:payload;
      try reflexivity.
    destruct (Schema.descriptor_child_type_id field)
      as [type_id|]; [|reflexivity].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|]; [|reflexivity].
    pose proof (parsed_generated_message_child_has_strictly_smaller_region
      bytes region_start region_end program spans span field
      parsed member selected payload) as strict.
    assert (child_enough :
      Wire.span_end span - Wire.span_payload_start span < fuel) by lia.
    f_equal. f_equal.
    change (decode_region (S fuel) bytes child_program
      (Wire.span_payload_start span) (Wire.span_end span) =
      decode_region fuel bytes child_program
      (Wire.span_payload_start span) (Wire.span_end span)).
    apply IH. exact child_enough.
Qed.

Theorem root_byte_fuel_is_saturated :
  forall extra bytes program,
    decode_region (S (length bytes) + extra) bytes program
      0 (length bytes) =
    decode_region (S (length bytes)) bytes program
      0 (length bytes).
Proof.
  induction extra as [|extra IH]; intros bytes program.
  - now rewrite Nat.add_0_r.
  - replace (S (length bytes) + S extra)
      with (S (S (length bytes) + extra)) by lia.
    rewrite (decode_region_is_stable_above_its_byte_depth
      (S (length bytes) + extra) bytes program 0 (length bytes)).
    + apply IH.
    + simpl. lia.
Qed.

Definition decode_root (type_id : nat) (bytes : list nat)
    : option (list source_tree) :=
  match Schema.lookup_projected_program type_id [] [] with
  | Some program => decode_region (S (length bytes)) bytes program 0 (length bytes)
  | None => None
  end.

Definition decode_raw_par (bytes : list nat) : option (list source_tree) :=
  decode_root 1 bytes.

Record string_map_entry_source := StringMapEntrySource {
  map_outer_span : Wire.field_span;
  map_outer_field : Schema.field_descriptor;
  map_value_type_id : nat;
  map_key_span : option Wire.field_span;
  map_value_span : option Wire.field_span
}.

Definition map_entry_field_is (tag : nat) (span : Wire.field_span) : bool :=
  (Wire.span_number span =? tag) &&
  match Wire.span_wire span with
  | Wire.WireLength => true
  | _ => false
  end.

Lemma map_entry_field_is_exact_tag_and_length_wire :
  forall tag span,
    map_entry_field_is tag span = true ->
    Wire.span_number span = tag /\
    Wire.span_wire span = Wire.WireLength.
Proof.
  intros tag span selected.
  unfold map_entry_field_is in selected.
  apply andb_true_iff in selected as [tag_match wire_match].
  apply Nat.eqb_eq in tag_match.
  destruct (Wire.span_wire span) eqn:wire;
    simpl in wire_match; try discriminate.
  now split.
Qed.

Definition parse_string_map_entry_spans (spans : list Wire.field_span)
    : option (option Wire.field_span * option Wire.field_span) :=
  match spans with
  | [] => Some (None, None)
  | [first] =>
      if map_entry_field_is 1 first then Some (Some first, None)
      else if map_entry_field_is 2 first then Some (None, Some first)
      else None
  | [first; second] =>
      if map_entry_field_is 1 first && map_entry_field_is 2 second
      then Some (Some first, Some second) else None
  | _ => None
  end.

Inductive canonical_string_map_entry_spans :
    list Wire.field_span -> option Wire.field_span ->
    option Wire.field_span -> Prop :=
| CanonicalMapEntryEmpty :
    canonical_string_map_entry_spans [] None None
| CanonicalMapEntryKey : forall key,
    map_entry_field_is 1 key = true ->
    canonical_string_map_entry_spans [key] (Some key) None
| CanonicalMapEntryValue : forall value,
    map_entry_field_is 2 value = true ->
    canonical_string_map_entry_spans [value] None (Some value)
| CanonicalMapEntryKeyValue : forall key value,
    map_entry_field_is 1 key = true ->
    map_entry_field_is 2 value = true ->
    canonical_string_map_entry_spans
      [key; value] (Some key) (Some value).

Theorem canonical_string_map_entry_has_at_most_two_fields :
  forall spans key value,
    canonical_string_map_entry_spans spans key value ->
    length spans <= 2.
Proof.
  intros spans key value shaped.
  now inversion shaped; subst; simpl; lia.
Qed.

Theorem parsed_string_map_entry_has_exact_constructor_shape :
  forall spans key value,
    parse_string_map_entry_spans spans = Some (key, value) ->
    canonical_string_map_entry_spans spans key value.
Proof.
  intros spans key value parsed.
  destruct spans as [|first [|second [|third rest]]];
    simpl in parsed; try discriminate.
  - inversion parsed; subst. constructor.
  - destruct (map_entry_field_is 1 first) eqn:first_key.
    + inversion parsed; subst.
      now apply CanonicalMapEntryKey.
    + destruct (map_entry_field_is 2 first) eqn:first_value;
        try discriminate.
      inversion parsed; subst.
      now apply CanonicalMapEntryValue.
  - destruct (map_entry_field_is 1 first &&
      map_entry_field_is 2 second) eqn:both; try discriminate.
    apply andb_true_iff in both as [first_key second_value].
    inversion parsed; subst.
    now apply CanonicalMapEntryKeyValue.
Qed.

Definition decode_string_map_entry_source (bytes : list nat)
    (program : Schema.generated_program) (node : source_tree)
    : option string_map_entry_source :=
  match node with
  | SourceAuxiliary span _ =>
      match source_descriptor program span with
      | Some field =>
          if Schema.payload_eqb (Schema.descriptor_payload field)
              Schema.StringMap then
            match Schema.descriptor_child_type_id field with
            | Some type_id =>
                match Wire.worklist_region bytes
                    (Wire.span_payload_start span) (Wire.span_end span) true with
                | Wire.Parsed entry_spans =>
                    option_map (fun entry =>
                      StringMapEntrySource span field type_id
                        (fst entry) (snd entry))
                      (parse_string_map_entry_spans entry_spans)
                | _ => None
                end
            | None => None
            end
          else None
      | None => None
      end
  | _ => None
  end.

Definition map_span_body (bytes : list nat) (span : Wire.field_span)
    : list nat :=
  firstn (Wire.span_end span - Wire.span_payload_start span)
    (skipn (Wire.span_payload_start span) bytes).

Lemma map_entry_source_uses_actual_generated_field :
  forall bytes program node entry,
    decode_string_map_entry_source bytes program node = Some entry ->
    source_descriptor program (map_outer_span entry) =
      Some (map_outer_field entry) /\
    Schema.descriptor_payload (map_outer_field entry) =
      Schema.StringMap /\
    Schema.descriptor_child_type_id (map_outer_field entry) =
      Some (map_value_type_id entry).
Proof.
  intros bytes program node entry decoded.
  destruct node as [span field|span field children|span field];
    unfold decode_string_map_entry_source in decoded;
    simpl in decoded; try discriminate.
  destruct (source_descriptor program span) as [selected|]
    eqn:resolved; [|discriminate].
  destruct (Schema.descriptor_payload selected) eqn:payload;
    simpl in decoded; try discriminate.
  destruct (Schema.descriptor_child_type_id selected)
    as [type_id|] eqn:child; [|discriminate].
  destruct (Wire.worklist_region bytes (Wire.span_payload_start span)
    (Wire.span_end span) true) as [spans| |]; try discriminate.
  destruct (parse_string_map_entry_spans spans) as [entry_spans|];
    [|discriminate].
  inversion decoded; subst entry; simpl.
  repeat split; assumption.
Qed.

Theorem decoded_string_map_entry_has_actual_constructor_shape :
  forall bytes program node entry,
    decode_string_map_entry_source bytes program node = Some entry ->
    exists span carried entry_spans,
      node = SourceAuxiliary span carried /\
      map_outer_span entry = span /\
      source_descriptor program span = Some (map_outer_field entry) /\
      Schema.descriptor_payload (map_outer_field entry) =
        Schema.StringMap /\
      Schema.descriptor_child_type_id (map_outer_field entry) =
        Some (map_value_type_id entry) /\
      Wire.worklist_region bytes (Wire.span_payload_start span)
        (Wire.span_end span) true = Wire.Parsed entry_spans /\
      canonical_string_map_entry_spans entry_spans
        (map_key_span entry) (map_value_span entry).
Proof.
  intros bytes program node entry decoded.
  destruct node as [span carried|span carried children|span carried];
    unfold decode_string_map_entry_source in decoded;
    simpl in decoded; try discriminate.
  destruct (source_descriptor program span) as [selected|]
    eqn:resolved; [|discriminate].
  destruct (Schema.descriptor_payload selected) eqn:payload;
    simpl in decoded; try discriminate.
  destruct (Schema.descriptor_child_type_id selected)
    as [type_id|] eqn:child; [|discriminate].
  destruct (Wire.worklist_region bytes (Wire.span_payload_start span)
    (Wire.span_end span) true) as [entry_spans| |]
    eqn:framed; try discriminate.
  destruct (parse_string_map_entry_spans entry_spans)
    as [[key value]|] eqn:parsed; [|discriminate].
  inversion decoded; subst entry; simpl.
  exists span, carried, entry_spans.
  repeat split; try assumption; try reflexivity.
  now apply (parsed_string_map_entry_has_exact_constructor_shape
    entry_spans key value).
Qed.

Definition source_map_entry_constructor_shape (bytes : list nat)
    (program : Schema.generated_program) (node : source_tree) : Prop :=
  exists entry span carried entry_spans,
    decode_string_map_entry_source bytes program node = Some entry /\
    node = SourceAuxiliary span carried /\
    map_outer_span entry = span /\
    source_descriptor program span = Some (map_outer_field entry) /\
    Schema.descriptor_payload (map_outer_field entry) =
      Schema.StringMap /\
    Schema.descriptor_child_type_id (map_outer_field entry) =
      Some (map_value_type_id entry) /\
    Wire.worklist_region bytes (Wire.span_payload_start span)
      (Wire.span_end span) true = Wire.Parsed entry_spans /\
    canonical_string_map_entry_spans entry_spans
      (map_key_span entry) (map_value_span entry).

Lemma decoded_string_map_entry_has_constructor_shape :
  forall bytes program node entry,
    decode_string_map_entry_source bytes program node = Some entry ->
    source_map_entry_constructor_shape bytes program node.
Proof.
  intros bytes program node entry decoded.
  destruct (decoded_string_map_entry_has_actual_constructor_shape
    bytes program node entry decoded)
    as [span [carried [entry_spans shape]]].
  exists entry, span, carried, entry_spans.
  now split.
Qed.

Lemma map_span_body_preserves_source_offset :
  forall bytes span index,
    index < Wire.span_end span - Wire.span_payload_start span ->
    nth_error (map_span_body bytes span) index =
      nth_error bytes (Wire.span_payload_start span + index).
Proof.
  intros bytes span index within.
  unfold map_span_body.
  rewrite nth_error_firstn, nth_error_skipn.
  apply Nat.ltb_lt in within.
  now rewrite within.
Qed.

Record typed_string_map_entry := TypedStringMapEntry {
  typed_map_outer_span : Wire.field_span;
  typed_map_outer_field : Schema.field_descriptor;
  typed_map_value_type_id : nat;
  typed_map_value_program : Schema.generated_program;
  typed_map_key_source : option (Wire.field_span * list nat);
  typed_map_value_source : option (Wire.field_span * list nat)
}.

Definition refine_string_map_entry (bytes : list nat)
    (program : Schema.generated_program) (node : source_tree)
    : option typed_string_map_entry :=
  match decode_string_map_entry_source bytes program node with
  | None => None
  | Some entry =>
      match Schema.lookup_projected_program (map_value_type_id entry) [] [] with
      | None => None
      | Some value_program =>
          Some (TypedStringMapEntry (map_outer_span entry)
            (map_outer_field entry) (map_value_type_id entry) value_program
            (option_map (fun span => (span, map_span_body bytes span))
              (map_key_span entry))
            (option_map (fun span => (span, map_span_body bytes span))
              (map_value_span entry)))
      end
  end.

Lemma parsed_string_map_entry_selects_key_and_value_roles :
  forall spans key value,
    parse_string_map_entry_spans spans = Some (key, value) ->
    (forall span, key = Some span -> map_entry_field_is 1 span = true) /\
    (forall span, value = Some span -> map_entry_field_is 2 span = true).
Proof.
  intros spans key value parsed.
  destruct spans as [|first [|second [|third rest]]];
    simpl in parsed; try discriminate.
  - inversion parsed; subst. split; intros span selected; discriminate.
  - destruct (map_entry_field_is 1 first) eqn:first_key.
    + inversion parsed; subst. split; intros span selected.
      * inversion selected; subst; exact first_key.
      * discriminate.
    + destruct (map_entry_field_is 2 first) eqn:first_value;
        try discriminate.
      inversion parsed; subst. split; intros span selected.
      * discriminate.
      * inversion selected; subst; exact first_value.
  - destruct (map_entry_field_is 1 first &&
      map_entry_field_is 2 second) eqn:both; try discriminate.
    apply andb_true_iff in both as [first_key second_value].
    inversion parsed; subst. split; intros span selected;
      inversion selected; subst; assumption.
Qed.

Lemma map_entry_source_selects_key_and_value_roles :
  forall bytes program node entry,
    decode_string_map_entry_source bytes program node = Some entry ->
    (forall span, map_key_span entry = Some span ->
      map_entry_field_is 1 span = true) /\
    (forall span, map_value_span entry = Some span ->
      map_entry_field_is 2 span = true).
Proof.
  intros bytes program node entry decoded.
  destruct node as [span field|span field children|span field];
    unfold decode_string_map_entry_source in decoded;
    simpl in decoded; try discriminate.
  destruct (source_descriptor program span) as [selected|];
    [|discriminate].
  destruct (Schema.descriptor_payload selected);
    simpl in decoded; try discriminate.
  destruct (Schema.descriptor_child_type_id selected) as [type_id|];
    [|discriminate].
  destruct (Wire.worklist_region bytes (Wire.span_payload_start span)
    (Wire.span_end span) true) as [spans| |]; try discriminate.
  destruct (parse_string_map_entry_spans spans) as [[key value]|]
    eqn:parsed; [|discriminate].
  inversion decoded; subst entry; simpl.
  now apply (parsed_string_map_entry_selects_key_and_value_roles
    spans key value).
Qed.

Theorem typed_map_entry_refines_source_selection :
  forall bytes program node typed,
    refine_string_map_entry bytes program node = Some typed ->
    exists entry,
      decode_string_map_entry_source bytes program node = Some entry /\
      typed_map_outer_span typed = map_outer_span entry /\
      typed_map_outer_field typed = map_outer_field entry /\
      typed_map_value_type_id typed = map_value_type_id entry /\
      Schema.lookup_projected_program (map_value_type_id entry) [] [] =
        Some (typed_map_value_program typed) /\
      source_descriptor program (typed_map_outer_span typed) =
        Some (typed_map_outer_field typed) /\
      Schema.descriptor_payload (typed_map_outer_field typed) =
        Schema.StringMap /\
      Schema.descriptor_child_type_id (typed_map_outer_field typed) =
        Some (typed_map_value_type_id typed) /\
      (forall span body,
        typed_map_key_source typed = Some (span, body) ->
        map_entry_field_is 1 span = true /\
        body = map_span_body bytes span) /\
      (forall span body,
        typed_map_value_source typed = Some (span, body) ->
        map_entry_field_is 2 span = true /\
        body = map_span_body bytes span).
Proof.
  intros bytes program node typed refined.
  unfold refine_string_map_entry in refined.
  destruct (decode_string_map_entry_source bytes program node)
    as [entry|] eqn:decoded; [|discriminate].
  destruct (Schema.lookup_projected_program (map_value_type_id entry) [] [])
    as [value_program|] eqn:looked_up; [|discriminate].
  inversion refined; subst typed; simpl.
  pose proof (map_entry_source_uses_actual_generated_field
    _ _ _ _ decoded) as [descriptor [payload child]].
  pose proof (map_entry_source_selects_key_and_value_roles
    _ _ _ _ decoded) as [key_role value_role].
  exists entry.
  repeat split; try assumption; try reflexivity.
  - destruct (map_key_span entry) as [key|] eqn:source_key;
      simpl in H; [|discriminate].
    inversion H; subst span body.
    apply key_role; reflexivity.
  - destruct (map_key_span entry) as [key|] eqn:source_key;
      simpl in H; [|discriminate].
    inversion H; subst span body.
    reflexivity.
  - destruct (map_value_span entry) as [value|] eqn:source_value;
      simpl in H; [|discriminate].
    inversion H; subst span body.
    apply value_role; reflexivity.
  - destruct (map_value_span entry) as [value|] eqn:source_value;
      simpl in H; [|discriminate].
    inversion H; subst span body.
    reflexivity.
Qed.

Definition byte_between (lower upper byte : nat) : bool :=
  (lower <=? byte) && (byte <=? upper).

Definition utf8_three_second (first second : nat) : bool :=
  if first =? 224 then byte_between 160 191 second
  else if first =? 237 then byte_between 128 159 second
  else (byte_between 225 236 first || byte_between 238 239 first) &&
    byte_between 128 191 second.

Definition utf8_four_second (first second : nat) : bool :=
  if first =? 240 then byte_between 144 191 second
  else if first =? 244 then byte_between 128 143 second
  else byte_between 241 243 first && byte_between 128 191 second.

Fixpoint utf8_bytes (bytes : list nat) : bool :=
  match bytes with
  | [] => true
  | first :: rest =>
      if byte_between 0 127 first then utf8_bytes rest
      else if byte_between 194 223 first then
        match rest with
        | second :: suffix =>
            byte_between 128 191 second && utf8_bytes suffix
        | [] => false
        end
      else if byte_between 224 239 first then
        match rest with
        | second :: third :: suffix =>
            utf8_three_second first second &&
            byte_between 128 191 third && utf8_bytes suffix
        | _ => false
        end
      else if byte_between 240 244 first then
        match rest with
        | second :: third :: fourth :: suffix =>
            utf8_four_second first second &&
            byte_between 128 191 third &&
            byte_between 128 191 fourth && utf8_bytes suffix
        | _ => false
        end
      else false
  end.

Definition indexed_utf8_three_second (first second : nat) : bool :=
  if first =? 224 then byte_between 160 191 second
  else if byte_between 225 236 first then byte_between 128 191 second
  else if first =? 237 then byte_between 128 159 second
  else if byte_between 238 239 first then byte_between 128 191 second
  else false.

Definition indexed_utf8_four_second (first second : nat) : bool :=
  if first =? 240 then byte_between 144 191 second
  else if byte_between 241 243 first then byte_between 128 191 second
  else if first =? 244 then byte_between 128 143 second
  else false.

Fixpoint indexed_scan_utf8_accepts (bytes : list nat) : bool :=
  match bytes with
  | [] => true
  | first :: rest =>
      if byte_between 0 127 first then indexed_scan_utf8_accepts rest
      else if byte_between 194 223 first then
        match rest with
        | second :: suffix =>
            byte_between 128 191 second &&
              indexed_scan_utf8_accepts suffix
        | [] => false
        end
      else if byte_between 224 239 first then
        match rest with
        | second :: third :: suffix =>
            indexed_utf8_three_second first second &&
            byte_between 128 191 third &&
              indexed_scan_utf8_accepts suffix
        | _ => false
        end
      else if byte_between 240 244 first then
        match rest with
        | second :: third :: fourth :: suffix =>
            indexed_utf8_four_second first second &&
            byte_between 128 191 third &&
            byte_between 128 191 fourth &&
              indexed_scan_utf8_accepts suffix
        | _ => false
        end
      else false
  end.

Lemma indexed_utf8_three_second_agrees_with_compact :
  forall first second,
    indexed_utf8_three_second first second =
      utf8_three_second first second.
Proof.
  intros first second.
  unfold indexed_utf8_three_second, utf8_three_second.
  destruct (first =? 224) eqn:first_e0; [reflexivity|].
  destruct (first =? 237) eqn:first_ed.
  - apply Nat.eqb_eq in first_ed. subst first. reflexivity.
  - destruct (byte_between 225 236 first);
      destruct (byte_between 238 239 first);
      destruct (byte_between 128 191 second); reflexivity.
Qed.

Lemma indexed_utf8_four_second_agrees_with_compact :
  forall first second,
    indexed_utf8_four_second first second =
      utf8_four_second first second.
Proof.
  intros first second.
  unfold indexed_utf8_four_second, utf8_four_second.
  destruct (first =? 240) eqn:first_f0; [reflexivity|].
  destruct (first =? 244) eqn:first_f4.
  - apply Nat.eqb_eq in first_f4. subst first. reflexivity.
  - destruct (byte_between 241 243 first);
      destruct (byte_between 128 191 second); reflexivity.
Qed.

Theorem indexed_scan_utf8_accepts_exactly_compact_utf8 :
  forall bytes, indexed_scan_utf8_accepts bytes = utf8_bytes bytes.
Proof.
  fix IH 1.
  intros [|first rest]; [reflexivity|].
  simpl.
  destruct (byte_between 0 127 first); [apply IH|].
  destruct (byte_between 194 223 first).
  - destruct rest as [|second suffix]; [reflexivity|].
    now rewrite IH.
  - destruct (byte_between 224 239 first).
    + destruct rest as [|second [|third suffix]]; try reflexivity.
      rewrite indexed_utf8_three_second_agrees_with_compact.
      now rewrite IH.
    + destruct (byte_between 240 244 first); [|reflexivity].
      destruct rest as [|second [|third [|fourth suffix]]];
        try reflexivity.
      rewrite indexed_utf8_four_second_agrees_with_compact.
      now rewrite IH.
Qed.

Record typed_map_return (A : Type) := TypedMapReturn {
  returned_map_key : list nat;
  returned_map_key_span : option Wire.field_span;
  returned_map_par : option A;
  returned_map_par_span : option Wire.field_span
}.

Definition reduce_map_key (source : option (Wire.field_span * list nat))
    : option (list nat * option Wire.field_span) :=
  match source with
  | None => Some ([], None)
  | Some (span, key) =>
      if utf8_bytes key &&
          match key with [] => false | _ => true end
      then Some (key, Some span) else None
  end.

Definition reduce_map_par {A : Type}
    (reduce_par : list nat -> option (bool * A))
    (source : option (Wire.field_span * list nat))
    : option (option A * option Wire.field_span) :=
  match source with
  | None => Some (None, None)
  | Some (span, body) =>
      match reduce_par body with
      | Some (false, value) => Some (Some value, Some span)
      | _ => None
      end
  end.

Definition reduce_typed_map_entry {A : Type}
    (reduce_par : list nat -> option (bool * A))
    (entry : typed_string_map_entry) : option (typed_map_return A) :=
  match reduce_map_key (typed_map_key_source entry),
      reduce_map_par reduce_par (typed_map_value_source entry) with
  | Some (key, key_span), Some (value, value_span) =>
      Some (TypedMapReturn A key key_span value value_span)
  | _, _ => None
  end.

Lemma reduce_map_key_preserves_source_and_omits_default :
  forall source key span,
    reduce_map_key source = Some (key, span) ->
    match source with
    | None => key = [] /\ span = None
    | Some (source_span, body) =>
        utf8_bytes body = true /\ body <> [] /\
        key = body /\ span = Some source_span
    end.
Proof.
  intros [[source_span body]|] key span reduced; simpl in reduced.
  - destruct (utf8_bytes body &&
      match body with [] => false | _ => true end) eqn:accepted;
      [|discriminate].
    apply andb_true_iff in accepted as [valid nonempty].
    inversion reduced; subst key span; simpl.
    repeat split; try assumption; try reflexivity.
    destruct body; simpl in nonempty; discriminate.
  - inversion reduced; subst key span; simpl; auto.
Qed.

Theorem reduce_map_key_has_indexed_utf8_acceptance :
  forall span body,
    reduce_map_key (Some (span, body)) = Some (body, Some span) <->
      indexed_scan_utf8_accepts body = true /\ body <> [].
Proof.
  intros span body.
  rewrite indexed_scan_utf8_accepts_exactly_compact_utf8.
  split.
  - intro accepted.
    pose proof (reduce_map_key_preserves_source_and_omits_default
      (Some (span, body)) body (Some span) accepted)
      as [valid [nonempty [_ _]]].
    now split.
  - intros [valid nonempty].
    unfold reduce_map_key; simpl.
    rewrite valid.
    destruct body as [|first rest]; [contradiction|reflexivity].
Qed.

Lemma reduce_map_par_preserves_source_and_omits_default :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      source value span,
    reduce_map_par reduce_par source = Some (value, span) ->
    match source with
    | None => value = None /\ span = None
    | Some (source_span, body) =>
        exists decoded,
          reduce_par body = Some (false, decoded) /\
          value = Some decoded /\ span = Some source_span
    end.
Proof.
  intros A reduce_par [[source_span body]|] value span reduced;
    simpl in reduced.
  - destruct (reduce_par body) as [[semantic_default decoded]|]
      eqn:decoded_source; [|discriminate].
    destruct semantic_default; [discriminate|].
    inversion reduced; subst value span; simpl.
    exists decoded. now repeat split.
  - inversion reduced; subst value span; simpl; auto.
Qed.

Theorem reduced_typed_map_return_matches_source :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      entry returned,
    reduce_typed_map_entry reduce_par entry = Some returned ->
    (match typed_map_key_source entry with
     | None => returned_map_key A returned = [] /\
         returned_map_key_span A returned = None
     | Some (span, body) =>
         utf8_bytes body = true /\ body <> [] /\
         returned_map_key A returned = body /\
         returned_map_key_span A returned = Some span
     end) /\
    (match typed_map_value_source entry with
     | None => returned_map_par A returned = None /\
         returned_map_par_span A returned = None
     | Some (span, body) =>
         exists decoded,
           reduce_par body = Some (false, decoded) /\
           returned_map_par A returned = Some decoded /\
           returned_map_par_span A returned = Some span
     end).
Proof.
  intros A reduce_par entry returned reduced.
  unfold reduce_typed_map_entry in reduced.
  destruct (reduce_map_key (typed_map_key_source entry))
    as [[key key_span]|] eqn:key_result; [|discriminate].
  destruct (reduce_map_par reduce_par (typed_map_value_source entry))
    as [[value value_span]|] eqn:value_result; [|discriminate].
  inversion reduced; subst returned; simpl.
  split.
  - exact (reduce_map_key_preserves_source_and_omits_default
      _ _ _ key_result).
  - exact (reduce_map_par_preserves_source_and_omits_default
      _ _ _ _ _ value_result).
Qed.

Theorem reduced_typed_map_return_uses_indexed_utf8_key :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      entry returned,
    reduce_typed_map_entry reduce_par entry = Some returned ->
    match typed_map_key_source entry with
    | None => returned_map_key A returned = [] /\
        returned_map_key_span A returned = None
    | Some (span, body) =>
        indexed_scan_utf8_accepts body = true /\ body <> [] /\
        returned_map_key A returned = body /\
        returned_map_key_span A returned = Some span
    end.
Proof.
  intros A reduce_par entry returned reduced.
  pose proof (reduced_typed_map_return_matches_source
    A reduce_par entry returned reduced) as [key_source _].
  destruct (typed_map_key_source entry) as [[span body]|];
    simpl in key_source |- *; [|exact key_source].
  rewrite indexed_scan_utf8_accepts_exactly_compact_utf8.
  exact key_source.
Qed.

Theorem reduced_refined_map_key_is_indexed_valid_source_window :
  forall (A : Type) bytes program node typed returned
      (reduce_par : list nat -> option (bool * A)) span body,
    refine_string_map_entry bytes program node = Some typed ->
    typed_map_key_source typed = Some (span, body) ->
    reduce_typed_map_entry reduce_par typed = Some returned ->
    map_entry_field_is 1 span = true /\
    indexed_scan_utf8_accepts (map_span_body bytes span) = true /\
    map_span_body bytes span <> [] /\
    returned_map_key A returned = map_span_body bytes span /\
    returned_map_key_span A returned = Some span.
Proof.
  intros A bytes program node typed returned reduce_par span body
    refined source_key reduced.
  pose proof (typed_map_entry_refines_source_selection
    bytes program node typed refined) as selected_source.
  destruct selected_source as [entry selected_source].
  do 8 (destruct selected_source as [_ selected_source]).
  destruct selected_source as [key_role _].
  pose proof (key_role span body source_key) as selected_key.
  pose proof (reduced_typed_map_return_uses_indexed_utf8_key
    A reduce_par typed returned reduced) as returned_key.
  rewrite source_key in returned_key.
  destruct selected_key as [role body_eq].
  destruct returned_key as [valid [nonempty [same_key same_span]]].
  subst body.
  repeat split; assumption.
Qed.

Definition map_key_return_guard {A : Type}
    (source : option (Wire.field_span * list nat))
    (returned : typed_map_return A) : Prop :=
  match source with
  | None => returned_map_key A returned = [] /\
      returned_map_key_span A returned = None
  | Some (span, body) =>
      indexed_scan_utf8_accepts body = true /\ body <> [] /\
      returned_map_key A returned = body /\
      returned_map_key_span A returned = Some span
  end.

Definition map_value_return_guard {A : Type}
    (reduce_par : list nat -> option (bool * A))
    (source : option (Wire.field_span * list nat))
    (returned : typed_map_return A) : Prop :=
  match source with
  | None => returned_map_par A returned = None /\
      returned_map_par_span A returned = None
  | Some (span, body) =>
      exists value, reduce_par body = Some (false, value) /\
        returned_map_par A returned = Some value /\
        returned_map_par_span A returned = Some span
  end.

Lemma map_key_guard_reconstructs_reduction :
  forall (A : Type) source (returned : typed_map_return A),
    map_key_return_guard source returned ->
    reduce_map_key source = Some
      (returned_map_key A returned, returned_map_key_span A returned).
Proof.
  intros A [[span body]|] returned guarded;
    unfold map_key_return_guard in guarded; simpl in guarded.
  - destruct guarded as [valid [nonempty [same_key same_span]]].
    rewrite indexed_scan_utf8_accepts_exactly_compact_utf8 in valid.
    unfold reduce_map_key; simpl.
    rewrite valid.
    destruct body as [|first rest]; [contradiction|].
    now rewrite same_key, same_span.
  - destruct guarded as [same_key same_span].
    simpl. now rewrite same_key, same_span.
Qed.

Lemma map_value_guard_reconstructs_reduction :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      source (returned : typed_map_return A),
    map_value_return_guard reduce_par source returned ->
    reduce_map_par reduce_par source = Some
      (returned_map_par A returned, returned_map_par_span A returned).
Proof.
  intros A reduce_par [[span body]|] returned guarded;
    unfold map_value_return_guard in guarded; simpl in guarded.
  - destruct guarded as [value [accepted [same_value same_span]]].
    unfold reduce_map_par; simpl.
    rewrite accepted. now rewrite same_value, same_span.
  - destruct guarded as [same_value same_span].
    simpl. now rewrite same_value, same_span.
Qed.

Theorem reduce_typed_map_entry_exact_guarded_acceptance :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      entry returned,
    reduce_typed_map_entry reduce_par entry = Some returned <->
      map_key_return_guard (typed_map_key_source entry) returned /\
      map_value_return_guard reduce_par
        (typed_map_value_source entry) returned.
Proof.
  intros A reduce_par entry returned.
  split.
  - intro reduced.
    pose proof (reduced_typed_map_return_matches_source
      A reduce_par entry returned reduced) as [key_guard value_guard].
    split.
    + unfold map_key_return_guard.
      destruct (typed_map_key_source entry) as [[span body]|];
        simpl in key_guard |- *; [|exact key_guard].
      rewrite indexed_scan_utf8_accepts_exactly_compact_utf8.
      exact key_guard.
    + exact value_guard.
  - intros [key_guard value_guard].
    pose proof (map_key_guard_reconstructs_reduction
      A (typed_map_key_source entry) returned key_guard) as key_reduced.
    pose proof (map_value_guard_reconstructs_reduction
      A reduce_par (typed_map_value_source entry) returned
      value_guard) as value_reduced.
    unfold reduce_typed_map_entry.
    rewrite key_reduced, value_reduced.
    destruct returned. reflexivity.
Qed.

Definition reduce_typed_map_entry_via_source_child {A : Type}
    (bytes : list nat)
    (reduce_child : list nat -> Schema.generated_program ->
      list source_tree -> option (bool * A))
    (typed : typed_string_map_entry)
    : option (typed_map_return (list source_tree * A)) :=
  match reduce_map_key (typed_map_key_source typed) with
  | None => None
  | Some (key, key_span) =>
      match typed_map_value_source typed with
      | None => Some (TypedMapReturn (list source_tree * A)
          key key_span None None)
      | Some (span, _) =>
          match decode_region (S (length bytes)) bytes
              (typed_map_value_program typed)
              (Wire.span_payload_start span) (Wire.span_end span) with
          | None => None
          | Some child_nodes =>
              match reduce_child bytes (typed_map_value_program typed)
                  child_nodes with
              | Some (false, value) =>
                  Some (TypedMapReturn (list source_tree * A)
                    key key_span (Some (child_nodes, value)) (Some span))
              | _ => None
              end
          end
      end
  end.

Definition reduce_refined_map_entry_via_source_child {A : Type}
    (bytes : list nat) (program : Schema.generated_program)
    (reduce_child : list nat -> Schema.generated_program ->
      list source_tree -> option (bool * A))
    (node : source_tree)
    : option (typed_map_return (list source_tree * A)) :=
  match refine_string_map_entry bytes program node with
  | None => None
  | Some typed => reduce_typed_map_entry_via_source_child
      bytes reduce_child typed
  end.

Theorem refined_map_value_handoff_has_exact_generated_source :
  forall (A : Type) bytes program node
      (reduce_child : list nat -> Schema.generated_program ->
        list source_tree -> option (bool * A)) returned span,
    reduce_refined_map_entry_via_source_child bytes program
      reduce_child node = Some returned ->
    returned_map_par_span (list source_tree * A) returned = Some span ->
    exists typed body child_nodes value,
      refine_string_map_entry bytes program node = Some typed /\
      typed_map_value_source typed = Some (span, body) /\
      body = map_span_body bytes span /\
      source_descriptor program (typed_map_outer_span typed) =
        Some (typed_map_outer_field typed) /\
      Schema.descriptor_payload (typed_map_outer_field typed) =
        Schema.StringMap /\
      Schema.descriptor_child_type_id (typed_map_outer_field typed) =
        Some (typed_map_value_type_id typed) /\
      Schema.lookup_projected_program
        (typed_map_value_type_id typed) [] [] =
          Some (typed_map_value_program typed) /\
      decode_region (S (length bytes)) bytes
        (typed_map_value_program typed)
        (Wire.span_payload_start span) (Wire.span_end span) =
          Some child_nodes /\
      Forall (source_node_matches_generated_descriptor
        (typed_map_value_program typed)) child_nodes /\
      reduce_child bytes (typed_map_value_program typed)
        child_nodes = Some (false, value) /\
      returned_map_par (list source_tree * A) returned =
        Some (child_nodes, value).
Proof.
  intros A bytes program node reduce_child returned span reduced present.
  unfold reduce_refined_map_entry_via_source_child in reduced.
  destruct (refine_string_map_entry bytes program node)
    as [typed|] eqn:refined; [|discriminate].
  unfold reduce_typed_map_entry_via_source_child in reduced.
  destruct (reduce_map_key (typed_map_key_source typed))
    as [[key key_span]|] eqn:key_reduced; [|discriminate].
  destruct (typed_map_value_source typed) as [[value_span body]|]
    eqn:value_source; [|inversion reduced; subst; discriminate].
  destruct (decode_region (S (length bytes)) bytes
    (typed_map_value_program typed)
    (Wire.span_payload_start value_span) (Wire.span_end value_span))
    as [child_nodes|] eqn:decoded; [|discriminate].
  destruct (reduce_child bytes (typed_map_value_program typed)
    child_nodes) as [[semantic_default value]|] eqn:child_reduced;
    [|discriminate].
  destruct semantic_default; [discriminate|].
  inversion reduced; subst returned; simpl in present.
  inversion present; subst value_span.
  pose proof (typed_map_entry_refines_source_selection
    bytes program node typed refined) as selected.
  destruct selected as [entry facts].
  do 3 (destruct facts as [_ facts]).
  destruct facts as [same_type facts].
  destruct facts as [looked_up facts].
  destruct facts as [descriptor facts].
  destruct facts as [payload facts].
  destruct facts as [child_type facts].
  destruct facts as [_ value_role].
  pose proof (value_role span body value_source) as [_ body_source].
  exists typed, body, child_nodes, value.
  split; [assumption || reflexivity|].
  split; [assumption || reflexivity|].
  split; [exact body_source|].
  split; [exact descriptor|].
  split; [exact payload|].
  split; [exact child_type|].
  split; [rewrite same_type; exact looked_up|].
  split; [assumption || reflexivity|].
  split.
  - eapply decoded_region_nodes_match_generated_source; exact decoded.
  - split; [exact child_reduced|reflexivity].
Qed.

Theorem source_par_child_decode_failure_refuses_map_value :
  forall (A : Type) bytes
      (reduce_child : list nat -> Schema.generated_program ->
        list source_tree -> option (bool * A)) typed span body,
    typed_map_value_source typed = Some (span, body) ->
    decode_region (S (length bytes)) bytes
      (typed_map_value_program typed)
      (Wire.span_payload_start span) (Wire.span_end span) = None ->
    reduce_typed_map_entry_via_source_child bytes reduce_child typed = None.
Proof.
  intros A bytes reduce_child typed span body source failed.
  unfold reduce_typed_map_entry_via_source_child.
  rewrite source, failed.
  destruct (reduce_map_key (typed_map_key_source typed))
    as [[key key_span]|]; reflexivity.
Qed.

Theorem source_par_child_semantic_default_refuses_map_value :
  forall (A : Type) bytes
      (reduce_child : list nat -> Schema.generated_program ->
        list source_tree -> option (bool * A)) typed span body
        child_nodes value,
    typed_map_value_source typed = Some (span, body) ->
    decode_region (S (length bytes)) bytes
      (typed_map_value_program typed)
      (Wire.span_payload_start span) (Wire.span_end span) =
        Some child_nodes ->
    reduce_child bytes (typed_map_value_program typed)
      child_nodes = Some (true, value) ->
    reduce_typed_map_entry_via_source_child bytes reduce_child typed = None.
Proof.
  intros A bytes reduce_child typed span body child_nodes value
    source decoded semantic_default.
  unfold reduce_typed_map_entry_via_source_child.
  rewrite source, decoded, semantic_default.
  destruct (reduce_map_key (typed_map_key_source typed))
    as [[key key_span]|]; reflexivity.
Qed.

Definition generated_par_source_program : Schema.generated_program :=
  match Schema.lookup_projected_program 1 [] [] with
  | Some program => program
  | None => Schema.empty_program
  end.

Theorem generated_par_source_lookup_is_present :
  Schema.lookup_projected_program 1 [] [] =
    Some generated_par_source_program.
Proof. vm_compute. reflexivity. Qed.

Theorem generated_par_source_field_kinds :
  map (fun field => (Schema.descriptor_tag field,
    Schema.descriptor_repeated field, Schema.descriptor_payload field))
    (Schema.program_fields generated_par_source_program) =
    [(1, true, Schema.Message); (2, true, Schema.Message);
     (4, true, Schema.Message); (5, true, Schema.Message);
     (6, true, Schema.Message); (7, true, Schema.Message);
     (8, true, Schema.Message); (9, false, Schema.Scalar);
     (10, false, Schema.Scalar); (11, true, Schema.Message);
     (12, true, Schema.Message); (13, true, Schema.Message)].
Proof. vm_compute. reflexivity. Qed.

Theorem generated_par_component_field_tags :
  map Schema.descriptor_tag
    (filter (fun field =>
      Schema.descriptor_repeated field &&
      Schema.payload_eqb (Schema.descriptor_payload field) Schema.Message)
      (Schema.program_fields generated_par_source_program)) =
    [1; 2; 4; 5; 6; 7; 8; 11; 12; 13].
Proof. vm_compute. reflexivity. Qed.

Theorem generated_par_connective_field_is_varint_scalar :
  Schema.resolve_tag (Schema.program_fields generated_par_source_program)
    10 = Some (Schema.FieldDescriptor 10 8 Schema.Varint false
      Schema.Scalar None None).
Proof. vm_compute. reflexivity. Qed.

Definition source_par_component (node : source_tree) : bool :=
  Schema.descriptor_repeated (source_field node) &&
    Schema.payload_eqb (Schema.descriptor_payload (source_field node))
      Schema.Message.

Definition source_par_component_count (nodes : list source_tree) : nat :=
  length (filter source_par_component nodes).

Definition source_par_connective_node (node : source_tree) : bool :=
  (Schema.descriptor_tag (source_field node) =? 10) &&
    (Schema.descriptor_slot (source_field node) =? 8).

Definition source_par_true_body (bytes : list nat) (node : source_tree)
    : bool :=
  match source_body bytes node with
  | [1] => true
  | _ => false
  end.

Definition source_par_connective_used (bytes : list nat)
    (nodes : list source_tree) : option bool :=
  match find source_par_connective_node nodes with
  | None => Some false
  | Some node =>
      if source_par_true_body bytes node then Some true else None
  end.

Definition source_par_semantic_default (bytes : list nat)
    (nodes : list source_tree) : option bool :=
  option_map (fun connective =>
    (source_par_component_count nodes =? 0) && negb connective)
    (source_par_connective_used bytes nodes).

Lemma source_par_true_body_has_exact_original_byte :
  forall bytes node,
    source_par_true_body bytes node = true <-> source_body bytes node = [1].
Proof.
  intros bytes node.
  unfold source_par_true_body.
  destruct (source_body bytes node) as [|first [|second tail]]
    eqn:observed_body.
  - simpl. split; intro impossible; congruence.
  - destruct first as [|[|later]].
    + simpl. split; intro impossible; congruence.
    + simpl. split; intro accepted; reflexivity.
    + simpl. split; intro impossible; congruence.
  - destruct first as [|[|later]]; simpl;
      split; intro impossible; congruence.
Qed.

Theorem source_par_semantic_default_is_exactly_empty :
  forall bytes nodes,
    source_par_semantic_default bytes nodes = Some true <->
    source_par_component_count nodes = 0 /\
    find source_par_connective_node nodes = None.
Proof.
  intros bytes nodes.
  unfold source_par_semantic_default, source_par_connective_used.
  destruct (find source_par_connective_node nodes) as [node|]
    eqn:selected.
  - destruct (source_par_true_body bytes node).
    + simpl. rewrite andb_false_r.
      split; intro accepted; [discriminate|].
      destruct accepted as [_ absent]. congruence.
    + simpl. split; intro accepted; [discriminate|].
      destruct accepted as [_ absent]. congruence.
  - simpl. rewrite andb_true_r. split.
    + intro accepted.
      injection accepted as empty.
      apply Nat.eqb_eq in empty.
      now split.
    + intros [empty _].
      now rewrite empty.
Qed.

Theorem source_par_invalid_connective_body_refuses_summary :
  forall bytes nodes node,
    find source_par_connective_node nodes = Some node ->
    source_body bytes node <> [1] ->
    source_par_semantic_default bytes nodes = None.
Proof.
  intros bytes nodes node selected invalid.
  unfold source_par_semantic_default, source_par_connective_used.
  rewrite selected.
  destruct (source_par_true_body bytes node) eqn:valid; [|reflexivity].
  apply (proj1 (source_par_true_body_has_exact_original_byte bytes node))
    in valid.
  contradiction.
Qed.

Lemma source_par_semantic_nondefault_has_observation :
  forall bytes nodes,
    source_par_semantic_default bytes nodes = Some false ->
    source_par_component_count nodes > 0 \/
    source_par_connective_used bytes nodes = Some true.
Proof.
  intros bytes nodes accepted.
  unfold source_par_semantic_default in accepted.
  destruct (source_par_connective_used bytes nodes)
    as [connective|] eqn:observed; [|discriminate].
  destruct connective.
  - right. reflexivity.
  - left. simpl in accepted.
    destruct (source_par_component_count nodes =? 0)
      eqn:empty; simpl in accepted; [discriminate|].
    apply Nat.eqb_neq in empty. lia.
Qed.

Lemma source_par_connective_true_has_original_body :
  forall bytes nodes,
    source_par_connective_used bytes nodes = Some true ->
    exists node,
      find source_par_connective_node nodes = Some node /\
      source_body bytes node = [1].
Proof.
  intros bytes nodes accepted.
  unfold source_par_connective_used in accepted.
  destruct (find source_par_connective_node nodes) as [node|]
    eqn:selected; [|discriminate].
  destruct (source_par_true_body bytes node) eqn:valid;
    [|discriminate].
  exists node. split; [reflexivity|].
  apply (proj1 (source_par_true_body_has_exact_original_byte bytes node)).
  exact valid.
Qed.

Lemma selected_par_connective_is_source_member :
  forall nodes node,
    find source_par_connective_node nodes = Some node ->
    In node nodes /\ source_par_connective_node node = true.
Proof.
  intros nodes node.
  induction nodes as [|first rest IH]; simpl; intro selected;
    [discriminate|].
  destruct (source_par_connective_node first) eqn:matches.
  - inversion selected; subst node.
    split; [left; reflexivity|exact matches].
  - destruct (IH selected) as [member matches_node].
    split; [right; exact member|exact matches_node].
Qed.

Theorem decoded_par_nondefault_has_original_source_witness :
  forall fuel bytes region_start region_end nodes,
    decode_region fuel bytes generated_par_source_program
      region_start region_end = Some nodes ->
    source_par_semantic_default bytes nodes = Some false ->
    source_par_component_count nodes > 0 \/
    exists node,
      In node nodes /\
      source_descriptor generated_par_source_program
        (source_span node) = Some (source_field node) /\
      source_par_connective_node node = true /\
      source_body bytes node = [1].
Proof.
  intros fuel bytes region_start region_end nodes decoded nondefault.
  destruct (source_par_semantic_nondefault_has_observation
    bytes nodes nondefault) as [components|connective].
  - now left.
  - right.
    destruct (source_par_connective_true_has_original_body
      bytes nodes connective) as [node [selected body]].
    destruct (selected_par_connective_is_source_member
      nodes node selected) as [member selected_node].
    pose proof (decoded_region_nodes_match_generated_source
      fuel bytes generated_par_source_program region_start region_end
      nodes decoded) as generated.
    rewrite Forall_forall in generated.
    exists node.
    split; [exact member|].
    split; [exact (proj1 (generated node member))|].
    now split.
Qed.

Definition source_checked_par_child_reducer {A : Type}
    (reduce_semantics : list nat -> Schema.generated_program ->
      list source_tree -> option A)
    (bytes : list nat) (program : Schema.generated_program)
    (nodes : list source_tree) : option (bool * A) :=
  if Schema.program_family program =? 1 then
    match source_par_semantic_default bytes nodes,
        reduce_semantics bytes program nodes with
    | Some semantic_default, Some value => Some (semantic_default, value)
    | _, _ => None
    end
  else None.

Theorem source_checked_par_child_result_has_source_summary :
  forall (A : Type) reduce_semantics bytes program nodes
      semantic_default (value : A),
    source_checked_par_child_reducer reduce_semantics bytes
      program nodes = Some (semantic_default, value) ->
    Schema.program_family program = 1 /\
    source_par_semantic_default bytes nodes = Some semantic_default /\
    reduce_semantics bytes program nodes = Some value.
Proof.
  intros A reduce_semantics bytes program nodes semantic_default value
    reduced.
  unfold source_checked_par_child_reducer in reduced.
  destruct (Schema.program_family program =? 1) eqn:family;
    [|discriminate].
  apply Nat.eqb_eq in family.
  destruct (source_par_semantic_default bytes nodes)
    as [observed|] eqn:summary; [|discriminate].
  destruct (reduce_semantics bytes program nodes)
    as [result|] eqn:semantics; [|discriminate].
  inversion reduced; subst observed result.
  now repeat split.
Qed.

Theorem refined_map_value_with_source_checked_par_has_original_observation :
  forall (A : Type) reduce_semantics bytes program node
      (returned : typed_map_return (list source_tree * A)) span,
    reduce_refined_map_entry_via_source_child bytes program
      (source_checked_par_child_reducer reduce_semantics) node =
        Some returned ->
    returned_map_par_span (list source_tree * A) returned = Some span ->
    exists typed child_nodes value,
      refine_string_map_entry bytes program node = Some typed /\
      returned_map_par (list source_tree * A) returned =
        Some (child_nodes, value) /\
      Schema.program_family (typed_map_value_program typed) = 1 /\
      (source_par_component_count child_nodes > 0 \/
       exists child_node,
         In child_node child_nodes /\
         source_descriptor (typed_map_value_program typed)
           (source_span child_node) = Some (source_field child_node) /\
         source_par_connective_node child_node = true /\
         source_body bytes child_node = [1]).
Proof.
  intros A reduce_semantics bytes program node returned span
    reduced present.
  destruct (refined_map_value_handoff_has_exact_generated_source
    A bytes program node
    (source_checked_par_child_reducer reduce_semantics)
    returned span reduced present)
    as [typed [body [child_nodes [value facts]]]].
  assert (refined :
    refine_string_map_entry bytes program node = Some typed) by tauto.
  assert (matched : Forall (source_node_matches_generated_descriptor
    (typed_map_value_program typed)) child_nodes) by tauto.
  assert (child_reduced :
    source_checked_par_child_reducer reduce_semantics bytes
      (typed_map_value_program typed) child_nodes = Some (false, value))
    by tauto.
  assert (returned_child :
    returned_map_par (list source_tree * A) returned =
      Some (child_nodes, value)) by tauto.
  destruct (source_checked_par_child_result_has_source_summary
    A reduce_semantics bytes (typed_map_value_program typed)
    child_nodes false value child_reduced)
    as [family [summary _]].
  exists typed, child_nodes, value.
  repeat split; try assumption.
  destruct (source_par_semantic_nondefault_has_observation
    bytes child_nodes summary) as [components|connective].
  - now left.
  - right.
    destruct (source_par_connective_true_has_original_body
      bytes child_nodes connective) as [child_node [selected body_source]].
    destruct (selected_par_connective_is_source_member
      child_nodes child_node selected) as [member selected_node].
    rewrite Forall_forall in matched.
    exists child_node.
    split; [exact member|].
    split; [exact (proj1 (matched child_node member))|].
    now split.
Qed.

Theorem explicit_empty_map_key_is_refused :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      entry span,
    typed_map_key_source entry = Some (span, []) ->
    reduce_typed_map_entry reduce_par entry = None.
Proof.
  intros A reduce_par entry span source.
  unfold reduce_typed_map_entry, reduce_map_key.
  rewrite source. simpl.
  destruct (reduce_map_par reduce_par (typed_map_value_source entry));
    reflexivity.
Qed.

Theorem explicit_default_map_par_is_refused :
  forall (A : Type) (reduce_par : list nat -> option (bool * A))
      entry span body value,
    typed_map_value_source entry = Some (span, body) ->
    reduce_par body = Some (true, value) ->
    reduce_typed_map_entry reduce_par entry = None.
Proof.
  intros A reduce_par entry span body value source default.
  unfold reduce_typed_map_entry, reduce_map_par.
  rewrite source, default. simpl.
  destruct (reduce_map_key (typed_map_key_source entry))
    as [[key key_span]|]; reflexivity.
Qed.

Example utf8_map_key_accepts_ascii_and_valid_four_byte_scalar :
  utf8_bytes [97; 240; 144; 128; 128] = true.
Proof. vm_compute. reflexivity. Qed.

Example utf8_map_key_refuses_surrogate_overlong_and_incomplete :
  utf8_bytes [237; 160; 128] = false /\
  utf8_bytes [240; 128; 128; 128] = false /\
  utf8_bytes [226; 130] = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Fixpoint byte_lex_lt (left right : list nat) {struct left} : bool :=
  match left, right with
  | [], [] => false
  | [], _ :: _ => true
  | _ :: _, [] => false
  | first_left :: rest_left, first_right :: rest_right =>
      if first_left <? first_right then true
      else if first_right <? first_left then false
      else byte_lex_lt rest_left rest_right
  end.

Lemma byte_lex_lt_irreflexive :
  forall bytes, byte_lex_lt bytes bytes = false.
Proof.
  induction bytes as [|byte tail IH]; simpl; [reflexivity|].
  now rewrite Nat.ltb_irrefl, IH.
Qed.

Definition map_key_follows (prior : option (list nat))
    (key : list nat) : bool :=
  match prior with
  | None => true
  | Some previous => byte_lex_lt previous key
  end.

Fixpoint ordered_map_returns_after {A : Type} (prior : option (list nat))
    (returned : list (typed_map_return A)) : Prop :=
  match returned with
  | [] => True
  | head :: tail =>
      map_key_follows prior (returned_map_key A head) = true /\
      ordered_map_returns_after (Some (returned_map_key A head)) tail
  end.

Definition reduce_source_map_node {A : Type} (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (reduce_par : list nat -> option (bool * A))
    (node : source_tree) : option (typed_map_return A) :=
  match refine_string_map_entry bytes program node with
  | Some typed =>
      if Schema.descriptor_slot (typed_map_outer_field typed) =? slot
      then reduce_typed_map_entry reduce_par typed else None
  | None => None
  end.

Definition guarded_source_map_member {A : Type} (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (reduce_par : list nat -> option (bool * A))
    (node : source_tree) (returned : typed_map_return A) : Prop :=
  exists typed,
    refine_string_map_entry bytes program node = Some typed /\
    Schema.descriptor_slot (typed_map_outer_field typed) = slot /\
    map_key_return_guard (typed_map_key_source typed) returned /\
    map_value_return_guard reduce_par
      (typed_map_value_source typed) returned.

Theorem guarded_source_map_member_exact_reduction :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A)) node returned,
    reduce_source_map_node bytes program slot reduce_par node =
      Some returned <->
    guarded_source_map_member bytes program slot reduce_par node returned.
Proof.
  intros A bytes program slot reduce_par node returned.
  split.
  - intro reduced.
    unfold reduce_source_map_node in reduced.
    destruct (refine_string_map_entry bytes program node)
      as [typed|] eqn:refined; [|discriminate].
    destruct (Schema.descriptor_slot (typed_map_outer_field typed) =? slot)
      eqn:selected; [|discriminate].
    apply Nat.eqb_eq in selected.
    exists typed.
    split; [exact refined|].
    split; [exact selected|].
    exact ((proj1 (reduce_typed_map_entry_exact_guarded_acceptance
      A reduce_par typed returned)) reduced).
  - intros [typed [refined [selected [key_guard value_guard]]]].
    unfold reduce_source_map_node.
    rewrite refined, selected, Nat.eqb_refl.
    apply (proj2 (reduce_typed_map_entry_exact_guarded_acceptance
      A reduce_par typed returned)).
    now split.
Qed.

Fixpoint reduce_source_map_nodes_after {A : Type} (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (reduce_par : list nat -> option (bool * A))
    (prior : option (list nat)) (nodes : list source_tree)
    : option (list (typed_map_return A)) :=
  match nodes with
  | [] => Some []
  | node :: tail =>
      match reduce_source_map_node bytes program slot reduce_par node with
      | None => None
      | Some returned =>
          let key := returned_map_key A returned in
          if map_key_follows prior key then
            option_map (cons returned)
              (reduce_source_map_nodes_after bytes program slot reduce_par
                (Some key) tail)
          else None
      end
  end.

Definition reduce_source_map_nodes {A : Type} (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (reduce_par : list nat -> option (bool * A))
    (nodes : list source_tree) : option (list (typed_map_return A)) :=
  reduce_source_map_nodes_after bytes program slot reduce_par None nodes.

Lemma reduced_source_map_node_has_exact_typed_source :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A)) node returned,
    reduce_source_map_node bytes program slot reduce_par node =
      Some returned ->
    exists typed,
      refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (typed_map_outer_field typed) = slot /\
      reduce_typed_map_entry reduce_par typed = Some returned.
Proof.
  intros A bytes program slot reduce_par node returned reduced.
  unfold reduce_source_map_node in reduced.
  destruct (refine_string_map_entry bytes program node)
    as [typed|] eqn:source; [|discriminate].
  destruct (Schema.descriptor_slot (typed_map_outer_field typed) =? slot)
    eqn:selected; [|discriminate].
  apply Nat.eqb_eq in selected.
  exists typed. now repeat split.
Qed.

Theorem reduced_source_map_nodes_follow_strict_key_order :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A)) nodes prior returned,
    reduce_source_map_nodes_after bytes program slot reduce_par prior nodes =
      Some returned ->
    Forall2 (fun node result =>
      reduce_source_map_node bytes program slot reduce_par node = Some result)
      nodes returned /\
    ordered_map_returns_after prior returned.
Proof.
  intros A bytes program slot reduce_par nodes.
  induction nodes as [|node tail IH]; intros prior returned reduced;
    simpl in reduced.
  - inversion reduced; subst returned. split; constructor.
  - destruct (reduce_source_map_node bytes program slot reduce_par node)
      as [head|] eqn:head_source; [|discriminate].
    destruct (map_key_follows prior (returned_map_key A head))
      eqn:strict; [|discriminate].
    destruct (reduce_source_map_nodes_after bytes program slot reduce_par
      (Some (returned_map_key A head)) tail) as [suffix|]
      eqn:suffix_source; [|discriminate].
    inversion reduced; subst returned; clear reduced.
    specialize (IH (Some (returned_map_key A head)) suffix
      suffix_source) as [source_members ordered_suffix].
    split.
    + constructor; assumption.
    + simpl. now split.
Qed.

Theorem reduce_source_map_nodes_after_exact_guarded_acceptance :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A)) nodes prior returned,
    reduce_source_map_nodes_after bytes program slot reduce_par prior nodes =
      Some returned <->
    Forall2 (guarded_source_map_member bytes program slot reduce_par)
      nodes returned /\
    ordered_map_returns_after prior returned.
Proof.
  intros A bytes program slot reduce_par nodes prior returned.
  split.
  - intro reduced.
    pose proof (reduced_source_map_nodes_follow_strict_key_order
      A bytes program slot reduce_par nodes prior returned reduced)
      as [members ordered].
    split; [|exact ordered].
    clear reduced ordered.
    induction members as [|node result tail_nodes tail_results
      member rest IH].
    + constructor.
    + constructor; [|exact IH].
      exact ((proj1 (guarded_source_map_member_exact_reduction
        A bytes program slot reduce_par node result)) member).
  - intros [members ordered].
    revert prior ordered.
    induction members as [|node result tail_nodes tail_results
      member rest IH]; intros prior ordered.
    + reflexivity.
    + simpl in ordered.
      destruct ordered as [strict tail_order].
      simpl.
      rewrite (proj2 (guarded_source_map_member_exact_reduction
        A bytes program slot reduce_par node result) member).
      rewrite strict.
      rewrite (IH (Some (returned_map_key A result)) tail_order).
      reflexivity.
Qed.

Theorem reduced_source_map_sequence_has_typed_members_and_order :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A)) nodes returned,
    reduce_source_map_nodes bytes program slot reduce_par nodes =
      Some returned ->
    Forall2 (fun node result => exists typed,
      refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (typed_map_outer_field typed) = slot /\
      reduce_typed_map_entry reduce_par typed = Some result)
      nodes returned /\
    ordered_map_returns_after None returned.
Proof.
  intros A bytes program slot reduce_par nodes returned reduced.
  unfold reduce_source_map_nodes in reduced.
  pose proof (reduced_source_map_nodes_follow_strict_key_order
    A bytes program slot reduce_par nodes None returned reduced)
    as [source_members ordered].
  split; [|exact ordered].
  clear reduced ordered.
  induction source_members as [|node result nodes results member rest IH].
  - constructor.
  - constructor.
    + exact (reduced_source_map_node_has_exact_typed_source
        A bytes program slot reduce_par node result member).
    + exact IH.
Qed.

Theorem consecutive_duplicate_source_map_keys_are_refused :
  forall (A : Type) bytes program slot
      (reduce_par : list nat -> option (bool * A))
      first second rest first_result second_result key,
    reduce_source_map_node bytes program slot reduce_par first =
      Some first_result ->
    reduce_source_map_node bytes program slot reduce_par second =
      Some second_result ->
    returned_map_key A first_result = key ->
    returned_map_key A second_result = key ->
    reduce_source_map_nodes bytes program slot reduce_par
      (first :: second :: rest) = None.
Proof.
  intros A bytes program slot reduce_par first second rest
    first_result second_result key first_source second_source
    first_key second_key.
  unfold reduce_source_map_nodes.
  simpl.
  rewrite first_source, first_key.
  simpl.
  rewrite second_source, second_key.
  simpl.
  now rewrite byte_lex_lt_irreflexive.
Qed.

Definition source_map_node_in_slot (program : Schema.generated_program)
    (slot : nat) (node : source_tree)
    : bool :=
  match node with
  | SourceAuxiliary span _ =>
      match source_descriptor program span with
      | Some field =>
          Schema.payload_eqb (Schema.descriptor_payload field)
            Schema.StringMap && (Schema.descriptor_slot field =? slot)
      | None => false
      end
  | _ => false
  end.

Definition source_map_slot_witness (program : Schema.generated_program)
    (slot : nat) (node : source_tree) : Prop :=
  exists span carried selected,
    node = SourceAuxiliary span carried /\
    source_descriptor program span = Some selected /\
    Schema.descriptor_payload selected = Schema.StringMap /\
    Schema.descriptor_slot selected = slot.

Theorem source_map_node_in_slot_is_actual_generated_descriptor :
  forall program slot node,
    source_map_node_in_slot program slot node = true <->
    source_map_slot_witness program slot node.
Proof.
  intros program slot node. split.
  - intro selected_slot.
    destruct node as [span carried|span carried children|span carried];
      simpl in selected_slot; try discriminate.
    destruct (source_descriptor program span) as [selected|]
      eqn:resolved; [|discriminate].
    apply andb_true_iff in selected_slot as [payload slot_match].
    destruct (Schema.descriptor_payload selected) eqn:payload_kind;
      simpl in payload; try discriminate.
    apply Nat.eqb_eq in slot_match.
    exists span, carried, selected.
    repeat split; assumption || reflexivity.
  - intros [span [carried [selected
      [same [resolved [payload slot_match]]]]]].
    subst node. simpl. rewrite resolved, payload, slot_match.
    simpl. now rewrite Nat.eqb_refl.
Qed.

Theorem source_map_slot_filter_selects_exactly_generated_entries :
  forall program slot nodes node,
    In node (filter (source_map_node_in_slot program slot) nodes) <->
    In node nodes /\ source_map_slot_witness program slot node.
Proof.
  intros program slot nodes node.
  rewrite filter_In.
  rewrite source_map_node_in_slot_is_actual_generated_descriptor.
  tauto.
Qed.

Theorem decoded_generated_string_map_node_is_auxiliary :
  forall fuel bytes program region_start region_end nodes node field,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    In node nodes ->
    source_descriptor program (source_span node) = Some field ->
    Schema.descriptor_payload field = Schema.StringMap ->
    node = SourceAuxiliary (source_span node) field.
Proof.
  intros fuel bytes program region_start region_end nodes node field
    decoded present resolved payload.
  pose proof (decoded_region_nodes_match_generated_source
    fuel bytes program region_start region_end nodes decoded)
    as generated.
  rewrite Forall_forall in generated.
  destruct (generated node present) as [actual auxiliary_shape].
  rewrite actual in resolved.
  inversion resolved; subst field.
  now apply auxiliary_shape.
Qed.

Theorem decoded_region_map_slot_selects_every_generated_map_span :
  forall fuel bytes program region_start region_end nodes slot node,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    (In node (filter (source_map_node_in_slot program slot) nodes) <->
      In node nodes /\
      exists field,
        source_descriptor program (source_span node) = Some field /\
        Schema.descriptor_payload field = Schema.StringMap /\
        Schema.descriptor_slot field = slot).
Proof.
  intros fuel bytes program region_start region_end nodes slot node
    decoded.
  split.
  - intro included.
    apply source_map_slot_filter_selects_exactly_generated_entries
      in included as [present witness].
    destruct witness as [span [carried [selected
      [same [resolved [payload selected_slot]]]]]].
    subst node. split; [exact present|].
    exists selected. simpl. now repeat split.
  - intros [present [field [resolved [payload selected_slot]]]].
    pose proof (decoded_generated_string_map_node_is_auxiliary
      fuel bytes program region_start region_end nodes node field
      decoded present resolved payload) as auxiliary_shape.
    apply source_map_slot_filter_selects_exactly_generated_entries.
    split; [exact present|].
    exists (source_span node), field, field.
    repeat split; try assumption.
Qed.

Theorem source_map_slot_filter_preserves_pair_order :
  forall program slot before first between second after,
    source_map_slot_witness program slot first ->
    source_map_slot_witness program slot second ->
    filter (source_map_node_in_slot program slot)
      (before ++ first :: between ++ second :: after) =
    filter (source_map_node_in_slot program slot) before ++
      first :: filter (source_map_node_in_slot program slot) between ++
      second :: filter (source_map_node_in_slot program slot) after.
Proof.
  intros program slot before first between second after
    first_selected second_selected.
  apply source_map_node_in_slot_is_actual_generated_descriptor
    in first_selected.
  apply source_map_node_in_slot_is_actual_generated_descriptor
    in second_selected.
  repeat rewrite filter_app.
  cbn [filter].
  rewrite first_selected.
  repeat rewrite filter_app.
  cbn [filter].
  now rewrite second_selected.
Qed.

Definition decode_region_map_slot (fuel : nat) (bytes : list nat)
    (program : Schema.generated_program) (region_start region_end slot : nat)
    : option (list source_tree) :=
  option_map (filter (source_map_node_in_slot program slot))
    (decode_region fuel bytes program region_start region_end).

Theorem decoded_region_map_slot_has_exact_source_members :
  forall fuel bytes program region_start region_end slot selected,
    decode_region_map_slot fuel bytes program region_start region_end slot =
      Some selected ->
    exists nodes,
      decode_region fuel bytes program region_start region_end = Some nodes /\
      (forall node,
        In node selected <->
        In node nodes /\ source_map_slot_witness program slot node).
Proof.
  intros fuel bytes program region_start region_end slot selected decoded.
  unfold decode_region_map_slot in decoded.
  destruct (decode_region fuel bytes program region_start region_end)
    as [nodes|] eqn:source; [|discriminate].
  inversion decoded; subst selected.
  exists nodes. split; [reflexivity|].
  intro node.
  apply source_map_slot_filter_selects_exactly_generated_entries.
Qed.

Theorem decoded_region_map_slot_retains_source_pair_order :
  forall fuel bytes program region_start region_end slot
      before first between second after,
    decode_region fuel bytes program region_start region_end =
      Some (before ++ first :: between ++ second :: after) ->
    source_map_slot_witness program slot first ->
    source_map_slot_witness program slot second ->
    decode_region_map_slot fuel bytes program region_start region_end slot =
      Some (filter (source_map_node_in_slot program slot) before ++
        first :: filter (source_map_node_in_slot program slot) between ++
        second :: filter (source_map_node_in_slot program slot) after).
Proof.
  intros fuel bytes program region_start region_end slot
    before first between second after decoded first_map second_map.
  unfold decode_region_map_slot.
  rewrite decoded. simpl.
  now rewrite (source_map_slot_filter_preserves_pair_order
    program slot before first between second after first_map second_map).
Qed.

Definition reduce_root_map_slot {A : Type} (type_id slot : nat)
    (bytes : list nat) (reduce_par : list nat -> option (bool * A))
    : option (list (typed_map_return A)) :=
  match Schema.lookup_projected_program type_id [] [] with
  | None => None
  | Some program =>
      match decode_region (S (length bytes)) bytes program 0 (length bytes) with
      | None => None
      | Some nodes =>
          reduce_source_map_nodes bytes program slot reduce_par
            (filter (source_map_node_in_slot program slot) nodes)
      end
  end.

Theorem root_map_slot_reduction_preserves_source_and_order :
  forall (A : Type) type_id slot bytes
      (reduce_par : list nat -> option (bool * A)) returned,
    reduce_root_map_slot type_id slot bytes reduce_par = Some returned ->
    exists program nodes,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      decode_region (S (length bytes)) bytes program 0 (length bytes) =
        Some nodes /\
      Forall2 (fun node result => exists typed,
        refine_string_map_entry bytes program node = Some typed /\
        Schema.descriptor_slot (typed_map_outer_field typed) = slot /\
        reduce_typed_map_entry reduce_par typed = Some result)
        (filter (source_map_node_in_slot program slot) nodes) returned /\
      ordered_map_returns_after None returned.
Proof.
  intros A type_id slot bytes reduce_par returned reduced.
  unfold reduce_root_map_slot in reduced.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (decode_region (S (length bytes)) bytes program 0
    (length bytes)) as [nodes|] eqn:decoded; [|discriminate].
  exists program, nodes.
  split; [reflexivity|].
  split; [exact decoded|].
  exact (reduced_source_map_sequence_has_typed_members_and_order
    A bytes program slot reduce_par
    (filter (source_map_node_in_slot program slot) nodes) returned reduced).
Qed.

Theorem reduce_root_map_slot_exact_guarded_acceptance :
  forall (A : Type) type_id slot bytes
      (reduce_par : list nat -> option (bool * A)) returned,
    reduce_root_map_slot type_id slot bytes reduce_par = Some returned <->
    exists program nodes,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      decode_region (S (length bytes)) bytes program 0 (length bytes) =
        Some nodes /\
      Forall2 (guarded_source_map_member bytes program slot reduce_par)
        (filter (source_map_node_in_slot program slot) nodes) returned /\
      ordered_map_returns_after None returned.
Proof.
  intros A type_id slot bytes reduce_par returned.
  split.
  - intro reduced.
    unfold reduce_root_map_slot in reduced.
    destruct (Schema.lookup_projected_program type_id [] [])
      as [program|] eqn:looked_up; [|discriminate].
    destruct (decode_region (S (length bytes)) bytes program 0
      (length bytes)) as [nodes|] eqn:decoded; [|discriminate].
    exists program, nodes.
    split; [reflexivity|].
    split; [exact decoded|].
    unfold reduce_source_map_nodes in reduced.
    exact ((proj1 (reduce_source_map_nodes_after_exact_guarded_acceptance
      A bytes program slot reduce_par
      (filter (source_map_node_in_slot program slot) nodes) None returned))
      reduced).
  - intros [program [nodes [looked_up [decoded guarded]]]].
    unfold reduce_root_map_slot.
    rewrite looked_up, decoded.
    unfold reduce_source_map_nodes.
    exact ((proj2 (reduce_source_map_nodes_after_exact_guarded_acceptance
      A bytes program slot reduce_par
      (filter (source_map_node_in_slot program slot) nodes) None returned))
      guarded).
Qed.

Theorem reduced_root_map_slot_has_actual_entry_constructors :
  forall (A : Type) type_id slot bytes
      (reduce_par : list nat -> option (bool * A)) returned,
    reduce_root_map_slot type_id slot bytes reduce_par = Some returned ->
    exists program nodes,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      decode_region (S (length bytes)) bytes program 0 (length bytes) =
        Some nodes /\
      Forall (source_map_entry_constructor_shape bytes program)
        (filter (source_map_node_in_slot program slot) nodes).
Proof.
  intros A type_id slot bytes reduce_par returned reduced.
  destruct (root_map_slot_reduction_preserves_source_and_order
    A type_id slot bytes reduce_par returned reduced)
    as [program [nodes [looked_up [decoded [members ordered]]]]].
  exists program, nodes.
  split; [exact looked_up|].
  split; [exact decoded|].
  clear reduced ordered.
  induction members as [|node result tail_nodes results member rest IH].
  - constructor.
  - constructor; [|exact IH].
    destruct member as [typed [refined [_ _]]].
    unfold refine_string_map_entry in refined.
    destruct (decode_string_map_entry_source bytes program node)
      as [entry|] eqn:source; [|discriminate].
    exact (decoded_string_map_entry_has_constructor_shape
      bytes program node entry source).
Qed.

Definition generated_new_program : Schema.generated_program :=
  match Schema.lookup_projected_program 20 [] [] with
  | Some program => program
  | None => Schema.empty_program
  end.

Example new_injections_map_entry_has_source_key_and_value_spans :
  match decode_root 20 [34; 5; 10; 1; 97; 18; 0] with
  | Some [node] =>
      match decode_string_map_entry_source
          [34; 5; 10; 1; 97; 18; 0] generated_new_program node with
      | Some entry =>
          map_value_type_id entry = 1 /\
          match map_key_span entry, map_value_span entry with
          | Some key, Some value =>
              map_span_body [34; 5; 10; 1; 97; 18; 0] key = [97] /\
              map_span_body [34; 5; 10; 1; 97; 18; 0] value = []
          | _, _ => False
          end
      | None => False
      end
  | _ => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example typed_new_injections_entry_preserves_key_and_value_bodies :
  match decode_root 20 [34; 5; 10; 1; 97; 18; 0] with
  | Some [node] =>
      match refine_string_map_entry
          [34; 5; 10; 1; 97; 18; 0] generated_new_program node with
      | Some entry =>
          typed_map_value_type_id entry = 1 /\
          match typed_map_key_source entry, typed_map_value_source entry with
          | Some (_, key), Some (_, value) => key = [97] /\ value = []
          | _, _ => False
          end
      | None => False
      end
  | _ => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition raw_field_of_source (bytes : list nat) (node : source_tree)
    : option Schema.raw_field :=
  let span := source_span node in
  let field := source_field node in
  let body := source_body bytes node in
  let declared := Wire.span_end span - Wire.span_payload_start span in
  match node with
  | SourceScalar _ _ =>
      if Schema.payload_eqb (Schema.descriptor_payload field) Schema.Scalar
      then Some (Schema.RawField (Wire.span_number span)
        (wire_kind_of_span span) Schema.Scalar body declared None [] [])
      else None
  | SourceMessage _ _ _ =>
      if Schema.payload_eqb (Schema.descriptor_payload field) Schema.Message
      then option_map (fun type_id =>
        Schema.RawField (Wire.span_number span)
          (wire_kind_of_span span) Schema.Message body declared
          (Some type_id) [] []) (Schema.descriptor_child_type_id field)
      else None
  | SourceAuxiliary _ _ =>
      match Schema.descriptor_payload field with
      | Schema.StringMap =>
          option_map (fun type_id =>
            Schema.RawField (Wire.span_number span)
              (wire_kind_of_span span) Schema.StringMap body declared
              (Some type_id) [] [])
            (Schema.descriptor_child_type_id field)
      | Schema.PathMap =>
          match Schema.descriptor_child_type_id field with
          | None => Some (Schema.RawField (Wire.span_number span)
              (wire_kind_of_span span) Schema.PathMap body declared
              None [] [])
          | Some _ => None
          end
      | Schema.Scalar | Schema.Message => None
      end
  end.

Lemma raw_field_of_source_preserves_wire_and_body :
  forall bytes node raw,
    raw_field_of_source bytes node = Some raw ->
    Schema.raw_tag raw = Wire.span_number (source_span node) /\
    Schema.raw_wire raw = wire_kind_of_span (source_span node) /\
    Schema.raw_payload raw = Schema.descriptor_payload (source_field node) /\
    Schema.raw_body raw = source_body bytes node /\
    Schema.raw_declared_length raw =
      Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node).
Proof.
  intros bytes node raw converted.
  destruct node as [span field|span field children|span field];
    unfold raw_field_of_source in converted;
    simpl in converted;
    try discriminate.
  - destruct (Schema.descriptor_payload field) eqn:payload;
      simpl in converted; try discriminate.
    inversion converted; subst raw; simpl.
    now repeat split.
  - destruct (Schema.descriptor_payload field) eqn:payload;
      simpl in converted; try discriminate.
    destruct (Schema.descriptor_child_type_id field)
      as [type_id|] eqn:child; [|discriminate].
    inversion converted; subst raw; simpl.
    now repeat split.
  - destruct (Schema.descriptor_payload field) eqn:payload;
      simpl in converted; try discriminate.
    + destruct (Schema.descriptor_child_type_id field)
        as [type_id|]; [|discriminate].
      inversion converted; subst raw; simpl.
      now repeat split.
    + destruct (Schema.descriptor_child_type_id field)
        as [type_id|]; [discriminate|].
      inversion converted; subst raw; simpl.
      now repeat split.
Qed.

Lemma raw_message_field_uses_generated_child_type :
  forall bytes span field children raw,
    raw_field_of_source bytes (SourceMessage span field children) =
      Some raw ->
    Schema.raw_program_type_id raw =
      Schema.descriptor_child_type_id field.
Proof.
  intros bytes span field children raw converted.
  unfold raw_field_of_source in converted; simpl in converted.
  destruct (Schema.descriptor_payload field);
    simpl in converted; try discriminate.
  destruct (Schema.descriptor_child_type_id field)
    as [type_id|]; [|discriminate].
  inversion converted; reflexivity.
Qed.

Theorem raw_field_of_source_preserves_absolute_body_offsets :
  forall bytes node raw index,
    raw_field_of_source bytes node = Some raw ->
    Wire.span_payload_start (source_span node) <=
      Wire.span_end (source_span node) ->
    Wire.span_end (source_span node) <= length bytes ->
    index < Schema.raw_declared_length raw ->
    length (Schema.raw_body raw) = Schema.raw_declared_length raw /\
    nth_error (Schema.raw_body raw) index =
      nth_error bytes
        (Wire.span_payload_start (source_span node) + index).
Proof.
  intros bytes node raw index converted payload_end end_bytes within.
  pose proof (raw_field_of_source_preserves_wire_and_body
    _ _ _ converted) as [_ [_ [_ [body declared]]]].
  rewrite declared in within.
  rewrite body, declared.
  split.
  - now apply bounded_source_body_has_exact_length.
  - now apply source_body_byte_has_original_offset.
Qed.

Theorem decoded_raw_field_has_checked_original_length_and_offsets :
  forall fuel bytes program region_start region_end nodes node raw index,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    In node nodes ->
    raw_field_of_source bytes node = Some raw ->
    index < Schema.raw_declared_length raw ->
    length (Schema.raw_body raw) = Schema.raw_declared_length raw /\
    nth_error (Schema.raw_body raw) index =
      nth_error bytes
        (Wire.span_payload_start (source_span node) + index).
Proof.
  intros fuel bytes program region_start region_end nodes node raw index
    decoded member converted within.
  destruct (decoded_region_nodes_have_checked_source_windows
    fuel bytes program region_start region_end nodes decoded)
    as [end_within bounded].
  pose proof (source_forall_member _ _ _ _ bounded member)
    as [_ [payload_end end_region]].
  eapply raw_field_of_source_preserves_absolute_body_offsets;
    [exact converted|exact payload_end|lia|exact within].
Qed.

Theorem decoded_raw_field_has_valid_body_under_global_byte_guard :
  forall fuel bytes program region_start region_end nodes node raw,
    decode_region fuel bytes program region_start region_end = Some nodes ->
    In node nodes ->
    forallb Wire.byte_ok bytes = true ->
    raw_field_of_source bytes node = Some raw ->
    Schema.valid_raw_body raw = true.
Proof.
  intros fuel bytes program region_start region_end nodes node raw
    decoded member guarded converted.
  pose proof (decoded_source_node_has_exact_bounded_body
    fuel bytes program region_start region_end nodes node
    decoded member) as exact_length.
  pose proof (raw_field_of_source_preserves_wire_and_body
    bytes node raw converted) as [_ [_ [_ [body declared]]]].
  unfold Schema.valid_raw_body.
  rewrite body, declared, exact_length, Nat.eqb_refl.
  exact (source_body_inherits_global_byte_guard bytes node guarded).
Qed.

Theorem decoded_root_raw_field_passes_reference_body_guard :
  forall fuel bytes program nodes node raw,
    decode_region fuel bytes program 0 (length bytes) = Some nodes ->
    In node nodes ->
    raw_field_of_source bytes node = Some raw ->
    Schema.valid_raw_body raw = true.
Proof.
  intros fuel bytes program nodes node raw decoded member converted.
  eapply decoded_raw_field_has_valid_body_under_global_byte_guard;
    [exact decoded|exact member| |exact converted].
  eapply decoded_root_checks_every_source_byte; exact decoded.
Qed.

Lemma recursive_source_window_converted_body_is_valid :
  forall bytes region_start region_end node raw,
    recursive_source_window region_start region_end node ->
    region_end <= length bytes ->
    forallb Wire.byte_ok bytes = true ->
    raw_field_of_source bytes node = Some raw ->
    Schema.valid_raw_body raw = true.
Proof.
  intros bytes region_start region_end node raw
    window end_within guarded converted.
  pose proof (recursive_source_window_has_bounded_node
    region_start region_end node window) as bounded.
  unfold source_node_inside_region in bounded.
  destruct bounded as [_ [payload_end end_region]].
  pose proof (raw_field_of_source_preserves_wire_and_body
    bytes node raw converted) as [_ [_ [_ [body declared]]]].
  assert (exact_length : length (source_body bytes node) =
      Wire.span_end (source_span node) -
      Wire.span_payload_start (source_span node)).
  { apply bounded_source_body_has_exact_length; lia. }
  unfold Schema.valid_raw_body.
  rewrite body, declared, exact_length, Nat.eqb_refl.
  exact (source_body_inherits_global_byte_guard bytes node guarded).
Qed.

Theorem decoded_root_descendant_raw_field_passes_reference_body_guard :
  forall fuel bytes program nodes node descendant raw,
    decode_region fuel bytes program 0 (length bytes) = Some nodes ->
    In node nodes ->
    source_tree_descendant node descendant ->
    raw_field_of_source bytes descendant = Some raw ->
    Schema.valid_raw_body raw = true.
Proof.
  intros fuel bytes program nodes node descendant raw
    decoded member reached converted.
  pose proof (decoded_region_nodes_have_recursive_source_windows
    fuel bytes program 0 (length bytes) nodes decoded) as windows.
  pose proof (source_forall_member _ _ _ _ windows member)
    as root_window.
  destruct (recursive_source_window_reaches_descendant
    0 (length bytes) node descendant root_window reached)
    as [descendant_start [descendant_end [window end_within]]].
  eapply (recursive_source_window_converted_body_is_valid
    bytes descendant_start descendant_end descendant raw);
    [exact window|lia| |exact converted].
  eapply decoded_root_checks_every_source_byte; exact decoded.
Qed.

Fixpoint sequence_source_results {A : Type} (results : list (option A))
    : option (list A) :=
  match results with
  | [] => Some []
  | Some value :: rest =>
      option_map (cons value) (sequence_source_results rest)
  | None :: _ => None
  end.

Fixpoint raw_tree_of_source (fuel : nat) (bytes : list nat)
    (node : source_tree) : option (@tree Schema.raw_field) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match raw_field_of_source bytes node with
      | None => None
      | Some raw =>
          match node with
          | SourceScalar _ _ => Some (Node raw FNil)
          | SourceMessage _ _ children =>
              option_map (fun converted =>
                Node raw (forest_of_list converted))
                (sequence_source_results
                  (map (raw_tree_of_source smaller bytes) children))
          | SourceAuxiliary _ _ => Some (Node raw FNil)
          end
      end
  end.

Lemma raw_tree_of_source_preserves_root_label :
  forall fuel bytes node converted,
    raw_tree_of_source fuel bytes node = Some converted ->
    exists label children,
      converted = Node label children /\
      raw_field_of_source bytes node = Some label.
Proof.
  intros fuel bytes node converted result.
  destruct fuel as [|smaller]; [discriminate|].
  simpl in result.
  destruct (raw_field_of_source bytes node)
    as [label|] eqn:raw; [|discriminate].
  destruct node as [span field|span field descendants|span field];
    try discriminate.
  - inversion result; subst converted.
    exists label, FNil. now split.
  - destruct (sequence_source_results
      (map (raw_tree_of_source smaller bytes) descendants))
      as [children|]; [|discriminate].
    inversion result; subst converted.
    exists label, (forest_of_list children). now split.
  - inversion result; subst converted.
    exists label, FNil. now split.
Qed.

Theorem source_map_auxiliary_has_exact_raw_leaf :
  forall bytes span field type_id fuel,
    Schema.descriptor_payload field = Schema.StringMap ->
    Schema.descriptor_child_type_id field = Some type_id ->
    raw_tree_of_source (S fuel) bytes (SourceAuxiliary span field) =
      Some (Node (Schema.RawField (Wire.span_number span)
        (wire_kind_of_span span) Schema.StringMap
        (source_body bytes (SourceAuxiliary span field))
        (Wire.span_end span - Wire.span_payload_start span)
        (Some type_id) [] []) FNil).
Proof.
  intros bytes span field type_id fuel payload child.
  cbn [raw_tree_of_source raw_field_of_source source_span source_field].
  now rewrite payload, child.
Qed.

Theorem source_pathmap_auxiliary_has_exact_raw_leaf :
  forall bytes span field fuel,
    Schema.descriptor_payload field = Schema.PathMap ->
    Schema.descriptor_child_type_id field = None ->
    raw_tree_of_source (S fuel) bytes (SourceAuxiliary span field) =
      Some (Node (Schema.RawField (Wire.span_number span)
        (wire_kind_of_span span) Schema.PathMap
        (source_body bytes (SourceAuxiliary span field))
        (Wire.span_end span - Wire.span_payload_start span)
        None [] []) FNil).
Proof.
  intros bytes span field fuel payload child.
  cbn [raw_tree_of_source raw_field_of_source source_span source_field].
  now rewrite payload, child.
Qed.

Theorem decoded_source_map_entry_has_exact_raw_leaf :
  forall fuel bytes program nodes node entry,
    decode_region fuel bytes program 0 (length bytes) = Some nodes ->
    In node nodes ->
    decode_string_map_entry_source bytes program node = Some entry ->
    exists raw,
      raw_tree_of_source (S (length bytes)) bytes node =
        Some (Node raw FNil) /\
      Schema.raw_payload raw = Schema.StringMap /\
      Schema.raw_program_type_id raw = Some (map_value_type_id entry) /\
      Schema.raw_body raw = source_body bytes node /\
      Schema.valid_raw_body raw = true.
Proof.
  intros fuel bytes program nodes node entry
    decoded member refined.
  destruct (decoded_string_map_entry_has_actual_constructor_shape
    bytes program node entry refined)
    as [span [carried [entry_spans shape]]].
  destruct shape as [same [outer [selected [payload [child [_ _]]]]]].
  subst node.
  pose proof (decoded_region_nodes_match_generated_source
    fuel bytes program 0 (length bytes) nodes decoded) as generated.
  rewrite Forall_forall in generated.
  destruct (generated (SourceAuxiliary span carried) member)
    as [actual _].
  simpl in actual.
  rewrite actual in selected.
  inversion selected; subst carried.
  set (raw := Schema.RawField (Wire.span_number span)
    (wire_kind_of_span span) Schema.StringMap
    (source_body bytes (SourceAuxiliary span (map_outer_field entry)))
    (Wire.span_end span - Wire.span_payload_start span)
    (Some (map_value_type_id entry)) [] []).
  exists raw.
  repeat split; try reflexivity.
  - unfold raw.
    now apply source_map_auxiliary_has_exact_raw_leaf.
  - eapply decoded_root_raw_field_passes_reference_body_guard;
      [exact decoded|exact member|].
    unfold raw_field_of_source, raw.
    simpl. now rewrite payload, child.
Qed.

Definition raw_root_of_bytes (type_id : nat) (bytes : list nat)
    : option (@tree Schema.raw_field) :=
  match decode_root type_id bytes with
  | None => None
  | Some nodes =>
      option_map (fun children =>
        Node (Schema.RawField 0 Schema.LengthDelimited Schema.Message
          bytes (length bytes) (Some type_id) [] [])
          (forest_of_list children))
        (sequence_source_results
          (map (raw_tree_of_source (S (length bytes)) bytes) nodes))
  end.

Lemma raw_root_of_bytes_preserves_original_body :
  forall type_id bytes root,
    raw_root_of_bytes type_id bytes = Some root ->
    exists children,
      root = Node (Schema.RawField 0 Schema.LengthDelimited
        Schema.Message bytes (length bytes) (Some type_id) [] []) children.
Proof.
  intros type_id bytes root converted.
  unfold raw_root_of_bytes in converted.
  destruct (decode_root type_id bytes) as [nodes|]; [|discriminate].
  destruct (sequence_source_results
    (map (raw_tree_of_source (S (length bytes)) bytes) nodes))
    as [children|]; [|discriminate].
  inversion converted; subst root.
  eexists. reflexivity.
Qed.

Example nested_send_raw_tree_has_exact_source_bodies :
  match raw_root_of_bytes 1 [10; 2; 24; 1] with
  | Some (Node root
      (FCons (Node send (FCons (Node scalar FNil) FNil)) FNil)) =>
      Schema.raw_body root = [10; 2; 24; 1] /\
      Schema.raw_body send = [24; 1] /\
      Schema.raw_body scalar = [1] /\
      Schema.raw_program_type_id send = Some 15 /\
      Schema.raw_payload scalar = Schema.Scalar
  | _ => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example nested_send_payload_is_source_derived :
  match decode_raw_par [10; 2; 24; 1] with
  | Some [SourceMessage parent _ [SourceScalar child child_field]] =>
      Nat.eqb (Wire.span_number parent) 1 &&
      Nat.eqb (Wire.span_number child) 3 &&
      Nat.eqb (Wire.span_payload_start parent) 2 &&
      Nat.eqb (Wire.span_payload_start child) 3 &&
      match source_body [10; 2; 24; 1] (SourceScalar child child_field) with
      | [1] => true
      | _ => false
      end
  | _ => false
  end = true.
Proof. vm_compute. reflexivity. Qed.

Example nested_send_wrong_generated_wire_refuses :
  decode_raw_par [10; 2; 26; 0] = None.
Proof. vm_compute. reflexivity. Qed.

Example nested_send_truncated_payload_refuses :
  decode_raw_par [10; 2; 24] = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions resolved_source_descriptor_has_actual_tag_and_wire.
Print Assumptions bounded_source_body_has_exact_length.
Print Assumptions source_body_byte_has_original_offset.
Print Assumptions firstn_member_is_original.
Print Assumptions skipn_member_is_original.
Print Assumptions source_body_inherits_global_byte_guard.
Print Assumptions admitted_source_slot_obeys_generated_order.
Print Assumptions parsed_generated_message_child_has_strictly_smaller_region.
Print Assumptions root_generated_message_path_cannot_exhaust_byte_fuel.
Print Assumptions decode_region_is_stable_above_its_byte_depth.
Print Assumptions root_byte_fuel_is_saturated.
Print Assumptions raw_field_of_source_preserves_wire_and_body.
Print Assumptions raw_message_field_uses_generated_child_type.
Print Assumptions raw_field_of_source_preserves_absolute_body_offsets.
Print Assumptions raw_tree_of_source_preserves_root_label.
Print Assumptions source_map_auxiliary_has_exact_raw_leaf.
Print Assumptions source_pathmap_auxiliary_has_exact_raw_leaf.
Print Assumptions decoded_source_map_entry_has_exact_raw_leaf.
Print Assumptions raw_root_of_bytes_preserves_original_body.
Print Assumptions nested_send_raw_tree_has_exact_source_bodies.
Print Assumptions map_entry_source_uses_actual_generated_field.
Print Assumptions map_entry_field_is_exact_tag_and_length_wire.
Print Assumptions canonical_string_map_entry_has_at_most_two_fields.
Print Assumptions parsed_string_map_entry_has_exact_constructor_shape.
Print Assumptions decoded_string_map_entry_has_actual_constructor_shape.
Print Assumptions decoded_string_map_entry_has_constructor_shape.
Print Assumptions map_span_body_preserves_source_offset.
Print Assumptions new_injections_map_entry_has_source_key_and_value_spans.
Print Assumptions parsed_string_map_entry_selects_key_and_value_roles.
Print Assumptions map_entry_source_selects_key_and_value_roles.
Print Assumptions typed_map_entry_refines_source_selection.
Print Assumptions indexed_utf8_three_second_agrees_with_compact.
Print Assumptions indexed_utf8_four_second_agrees_with_compact.
Print Assumptions indexed_scan_utf8_accepts_exactly_compact_utf8.
Print Assumptions reduce_map_key_preserves_source_and_omits_default.
Print Assumptions reduce_map_key_has_indexed_utf8_acceptance.
Print Assumptions reduce_map_par_preserves_source_and_omits_default.
Print Assumptions reduced_typed_map_return_matches_source.
Print Assumptions reduced_typed_map_return_uses_indexed_utf8_key.
Print Assumptions reduced_refined_map_key_is_indexed_valid_source_window.
Print Assumptions map_key_guard_reconstructs_reduction.
Print Assumptions map_value_guard_reconstructs_reduction.
Print Assumptions reduce_typed_map_entry_exact_guarded_acceptance.
Print Assumptions refined_map_value_handoff_has_exact_generated_source.
Print Assumptions source_par_child_decode_failure_refuses_map_value.
Print Assumptions source_par_child_semantic_default_refuses_map_value.
Print Assumptions generated_par_source_lookup_is_present.
Print Assumptions generated_par_source_field_kinds.
Print Assumptions generated_par_component_field_tags.
Print Assumptions generated_par_connective_field_is_varint_scalar.
Print Assumptions source_par_true_body_has_exact_original_byte.
Print Assumptions source_par_semantic_default_is_exactly_empty.
Print Assumptions source_par_invalid_connective_body_refuses_summary.
Print Assumptions source_par_semantic_nondefault_has_observation.
Print Assumptions source_par_connective_true_has_original_body.
Print Assumptions selected_par_connective_is_source_member.
Print Assumptions decoded_par_nondefault_has_original_source_witness.
Print Assumptions source_checked_par_child_result_has_source_summary.
Print Assumptions refined_map_value_with_source_checked_par_has_original_observation.
Print Assumptions explicit_empty_map_key_is_refused.
Print Assumptions explicit_default_map_par_is_refused.
Print Assumptions utf8_map_key_accepts_ascii_and_valid_four_byte_scalar.
Print Assumptions utf8_map_key_refuses_surrogate_overlong_and_incomplete.
Print Assumptions byte_lex_lt_irreflexive.
Print Assumptions reduced_source_map_node_has_exact_typed_source.
Print Assumptions guarded_source_map_member_exact_reduction.
Print Assumptions reduced_source_map_nodes_follow_strict_key_order.
Print Assumptions reduce_source_map_nodes_after_exact_guarded_acceptance.
Print Assumptions reduced_source_map_sequence_has_typed_members_and_order.
Print Assumptions consecutive_duplicate_source_map_keys_are_refused.
Print Assumptions root_map_slot_reduction_preserves_source_and_order.
Print Assumptions reduce_root_map_slot_exact_guarded_acceptance.
Print Assumptions reduced_root_map_slot_has_actual_entry_constructors.
Print Assumptions source_map_node_in_slot_is_actual_generated_descriptor.
Print Assumptions source_map_slot_filter_selects_exactly_generated_entries.
Print Assumptions decoded_region_nodes_match_generated_source.
Print Assumptions decoded_source_step_preserves_region_bounds.
Print Assumptions successful_source_fold_preserves_region_bounds.
Print Assumptions decoded_region_nodes_have_checked_source_windows.
Print Assumptions decoded_region_nodes_have_recursive_source_windows.
Print Assumptions decoded_source_node_has_exact_bounded_body.
Print Assumptions decoded_root_checks_every_source_byte.
Print Assumptions decoded_raw_field_has_checked_original_length_and_offsets.
Print Assumptions decoded_raw_field_has_valid_body_under_global_byte_guard.
Print Assumptions decoded_root_raw_field_passes_reference_body_guard.
Print Assumptions decoded_root_descendant_raw_field_passes_reference_body_guard.
Print Assumptions decoded_generated_string_map_node_is_auxiliary.
Print Assumptions decoded_region_map_slot_selects_every_generated_map_span.
Print Assumptions source_map_slot_filter_preserves_pair_order.
Print Assumptions decoded_region_map_slot_has_exact_source_members.
Print Assumptions decoded_region_map_slot_retains_source_pair_order.
Print Assumptions typed_new_injections_entry_preserves_key_and_value_bodies.
Print Assumptions nested_send_payload_is_source_derived.
Print Assumptions nested_send_wrong_generated_wire_refuses.
Print Assumptions nested_send_truncated_payload_refuses.
