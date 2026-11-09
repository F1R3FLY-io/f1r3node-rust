From Stdlib Require Import List Arith Lia NArith Bool.
From StackSafePDA Require Import RPB3WireCursor RPB3GeneratedFieldRefinement
  RPB3RawRegionTree.

Import ListNotations.

Module Wire := RPB3WireCursor.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.

Definition region_slice (bytes : list nat) (base finish : nat) : list nat :=
  firstn (finish - base) (skipn base bytes).

Definition rebase_span (base : nat) (span : Wire.field_span)
    : Wire.field_span :=
  Wire.FieldSpan (Wire.span_number span) (Wire.span_wire span)
    (Wire.span_start span - base) (Wire.span_key_end span - base)
    (Wire.span_payload_start span - base) (Wire.span_end span - base)
    (Wire.span_varint_value span).

Fixpoint rebase_tree (base : nat) (node : Source.source_tree)
    : Source.source_tree :=
  match node with
  | Source.SourceScalar span field =>
      Source.SourceScalar (rebase_span base span) field
  | Source.SourceMessage span field children =>
      Source.SourceMessage (rebase_span base span) field
        (map (rebase_tree base) children)
  | Source.SourceAuxiliary span field =>
      Source.SourceAuxiliary (rebase_span base span) field
  end.

Definition rebase_accumulator (base : nat)
    (current : Source.decode_accumulator) : Source.decode_accumulator :=
  Source.DecodeAccumulator (Source.decode_last_slot current)
    (Source.decode_seen_slots current)
    (map (rebase_tree base) (Source.decode_reversed current)).

Lemma region_slice_nth_error :
  forall bytes base finish offset,
    base <= offset < finish ->
    nth_error (region_slice bytes base finish) (offset - base) =
      nth_error bytes offset.
Proof.
  intros bytes base finish offset [before inside].
  unfold region_slice.
  rewrite nth_error_firstn, nth_error_skipn.
  assert (within : (offset - base <? finish - base) = true).
  { apply Nat.ltb_lt. lia. }
  rewrite within.
  f_equal. lia.
Qed.

Lemma region_slice_length :
  forall bytes base finish,
    base <= finish <= length bytes ->
    length (region_slice bytes base finish) = finish - base.
Proof.
  intros bytes base finish [ordered bounded].
  unfold region_slice.
  rewrite length_firstn, length_skipn.
  rewrite Nat.min_l by lia.
  reflexivity.
Qed.

Lemma region_subslice_rebases :
  forall bytes base slice_end region_start region_end,
    base <= region_start /\ region_start <= region_end /\
      region_end <= slice_end ->
    firstn (region_end - base - (region_start - base))
      (skipn (region_start - base)
        (region_slice bytes base slice_end)) =
    region_slice bytes region_start region_end.
Proof.
  intros bytes base slice_end region_start region_end
    [start_bound [end_bound slice_bound]].
  unfold region_slice.
  replace (region_end - base - (region_start - base))
    with (region_end - region_start) by lia.
  rewrite skipn_firstn_comm, firstn_firstn, Nat.min_l by lia.
  rewrite skipn_skipn.
  replace (region_start - base + base)
    with region_start by lia.
  reflexivity.
Qed.

Lemma region_ok_rebases :
  forall bytes base slice_end region_start region_end,
    base <= region_start /\ region_start <= region_end /\
      region_end <= slice_end /\ slice_end <= length bytes ->
    Wire.region_ok (region_slice bytes base slice_end)
      (region_start - base) (region_end - base) =
    Wire.region_ok bytes region_start region_end.
Proof.
  intros bytes base slice_end region_start region_end
    [start_bound [end_bound [slice_bound bytes_bound]]].
  unfold Wire.region_ok.
  assert (local_order :
    (region_start - base <=? region_end - base) = true)
    by (apply Nat.leb_le; lia).
  assert (global_order : (region_start <=? region_end) = true)
    by (apply Nat.leb_le; lia).
  assert (global_length : (region_end <=? length bytes) = true)
    by (apply Nat.leb_le; lia).
  assert (local_length :
    (region_end - base <=?
      length (region_slice bytes base slice_end)) = true).
  { rewrite region_slice_length by lia.
    apply Nat.leb_le. lia. }
  rewrite local_order, global_order, global_length, local_length.
  cbn [andb].
  rewrite (region_subslice_rebases bytes base slice_end
    region_start region_end) by lia.
  reflexivity.
Qed.

Lemma rebase_span_preserves_number_and_wire :
  forall base span,
    Wire.span_number (rebase_span base span) = Wire.span_number span /\
    Wire.span_wire (rebase_span base span) = Wire.span_wire span /\
    Wire.span_varint_value (rebase_span base span) =
      Wire.span_varint_value span.
Proof. intros. repeat split; reflexivity. Qed.

Lemma source_body_rebases :
  forall bytes base finish node,
    base <= Wire.span_payload_start (Source.source_span node) ->
    Wire.span_payload_start (Source.source_span node) <=
      Wire.span_end (Source.source_span node) ->
    Wire.span_end (Source.source_span node) <= finish ->
    Source.source_body (region_slice bytes base finish)
      (rebase_tree base node) =
      Source.source_body bytes node.
Proof.
  intros bytes base finish node before payload_end after.
  destruct node as [span field|span field children|span field];
    cbn [Source.source_span] in before, payload_end, after;
    unfold Source.source_body;
    cbn [Source.source_span rebase_tree rebase_span
      Wire.span_payload_start Wire.span_end];
    apply region_subslice_rebases; lia.
Qed.

Lemma varint_groups_rebase :
  forall fuel bytes base slice_end region_end offset group value,
    base <= offset /\ offset <= region_end /\
      region_end <= slice_end ->
    Wire.varint_groups fuel (region_slice bytes base slice_end)
      (region_end - base) (offset - base) group value =
    option_map (fun result => (fst result, snd result - base))
      (Wire.varint_groups fuel bytes region_end offset group value).
Proof.
  induction fuel as [|fuel IH];
    intros bytes base slice_end region_end offset group value
      [before [bounded within_slice]];
    [reflexivity|].
  cbn [Wire.varint_groups].
  assert (same_guard :
    (offset - base <? region_end - base) = (offset <? region_end)).
  { destruct (offset - base <? region_end - base) eqn:local;
      destruct (offset <? region_end) eqn:original;
      try reflexivity;
      apply Nat.ltb_lt in local || apply Nat.ltb_ge in local;
      apply Nat.ltb_lt in original || apply Nat.ltb_ge in original;
      lia. }
  rewrite same_guard.
  destruct (offset <? region_end) eqn:inside; [|reflexivity].
  rewrite region_slice_nth_error.
  2: { apply Nat.ltb_lt in inside. lia. }
  destruct (nth_error bytes offset) as [byte|]; [|reflexivity].
  destruct (Wire.byte_ok byte); [|reflexivity].
  destruct ((group =? 9) && (1 <? byte)); [reflexivity|].
  destruct (byte <? 128).
  - destruct ((0 <? group) && (byte =? 0)); [reflexivity|].
    cbn [option_map fst snd].
    replace (S offset - base) with (S (offset - base)) by lia.
    reflexivity.
  - replace (S (offset - base)) with (S offset - base) by lia.
    apply IH. apply Nat.ltb_lt in inside. lia.
Qed.

Lemma canonical_varint_rebase :
  forall bytes base slice_end region_end offset,
    base <= offset /\ offset <= region_end /\
      region_end <= slice_end ->
    Wire.canonical_varint (region_slice bytes base slice_end)
      (region_end - base) (offset - base) =
    option_map (fun result => (fst result, snd result - base))
      (Wire.canonical_varint bytes region_end offset).
Proof.
  intros bytes base slice_end region_end offset bounds.
  unfold Wire.canonical_varint.
  now apply varint_groups_rebase.
Qed.

Lemma varint_groups_success_bounds :
  forall fuel bytes finish offset group value decoded next,
    Wire.varint_groups fuel bytes finish offset group value =
      Some (decoded, next) ->
    offset < next <= finish.
Proof.
  induction fuel as [|fuel IH];
    intros bytes finish offset group value decoded next parsed;
    [discriminate|].
  cbn [Wire.varint_groups] in parsed.
  destruct (offset <? finish) eqn:inside; [|discriminate].
  destruct (nth_error bytes offset) as [byte|]; [|discriminate].
  destruct (Wire.byte_ok byte); [|discriminate].
  destruct ((group =? 9) && (1 <? byte)); [discriminate|].
  destruct (byte <? 128).
  - destruct ((0 <? group) && (byte =? 0)); [discriminate|].
    inversion parsed; subst next.
    apply Nat.ltb_lt in inside. lia.
  - pose proof (IH bytes finish (S offset) (S group)
      (value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N
      decoded next parsed) as [lower upper].
    apply Nat.ltb_lt in inside. lia.
Qed.

Lemma canonical_varint_success_bounds :
  forall bytes finish offset decoded next,
    Wire.canonical_varint bytes finish offset =
      Some (decoded, next) ->
    offset < next <= finish.
Proof.
  intros bytes finish offset decoded next parsed.
  unfold Wire.canonical_varint in parsed.
  eapply varint_groups_success_bounds; exact parsed.
Qed.

Lemma framing_field_rebase :
  forall bytes base slice_end region_end offset last numeric_order,
    base <= offset /\ offset <= region_end /\
      region_end <= slice_end ->
    Wire.framing_field (region_slice bytes base slice_end)
      (region_end - base) (offset - base) last numeric_order =
    option_map (rebase_span base)
      (Wire.framing_field bytes region_end offset last numeric_order).
Proof.
  intros bytes base slice_end region_end offset last numeric_order
    [before [bounded within_slice]].
  unfold Wire.framing_field.
  rewrite canonical_varint_rebase by lia.
  destruct (Wire.canonical_varint bytes region_end offset)
    as [[key key_end]|] eqn:key_result; [|reflexivity].
  cbn [option_map fst snd].
  pose proof (canonical_varint_success_bounds bytes region_end offset
    key key_end key_result) as [key_progress key_bounded].
  destruct ((0 <? key / 8)%N && Wire.field_number_within_limit
    (key / 8)%N); [|reflexivity].
  destruct (negb numeric_order ||
    (last <=? N.to_nat (key / 8)%N)); [|reflexivity].
  destruct (N.to_nat (key mod 8)%N)
    as [|[|[|[|[|[|other]]]]]]; try reflexivity.
  - rewrite canonical_varint_rebase by lia.
    destruct (Wire.canonical_varint bytes region_end key_end)
      as [[value value_end]|] eqn:value_result; [|reflexivity].
    cbn [option_map fst snd rebase_span].
    reflexivity.
  - assert (same_guard :
      ((key_end - base + 8 <=? region_end - base) =
        (key_end + 8 <=? region_end))).
    { destruct (key_end - base + 8 <=? region_end - base) eqn:local;
        destruct (key_end + 8 <=? region_end) eqn:original;
        try reflexivity;
        apply Nat.leb_le in local || apply Nat.leb_gt in local;
        apply Nat.leb_le in original || apply Nat.leb_gt in original;
        lia. }
    rewrite same_guard.
    destruct (key_end + 8 <=? region_end); [|reflexivity].
    cbn [option_map]. unfold rebase_span.
    cbn [Wire.span_number Wire.span_wire Wire.span_start
      Wire.span_key_end Wire.span_payload_start Wire.span_end
      Wire.span_varint_value].
    replace (key_end + 8 - base) with (key_end - base + 8) by lia.
    reflexivity.
  - rewrite canonical_varint_rebase by lia.
    destruct (Wire.canonical_varint bytes region_end key_end)
      as [[payload_length body_start]|] eqn:length_result;
      [|reflexivity].
    cbn [option_map fst snd].
    pose proof (canonical_varint_success_bounds bytes region_end key_end
      payload_length body_start length_result)
      as [body_progress body_bounded].
    replace (region_end - base - (body_start - base))
      with (region_end - body_start) by lia.
    destruct (N.leb payload_length
      (N.of_nat (region_end - body_start))); [|reflexivity].
    cbn [option_map]. unfold rebase_span.
    cbn [Wire.span_number Wire.span_wire Wire.span_start
      Wire.span_key_end Wire.span_payload_start Wire.span_end
      Wire.span_varint_value].
    replace (body_start + N.to_nat payload_length - base)
      with (body_start - base + N.to_nat payload_length) by lia.
    reflexivity.
  - assert (same_guard :
      ((key_end - base + 4 <=? region_end - base) =
        (key_end + 4 <=? region_end))).
    { destruct (key_end - base + 4 <=? region_end - base) eqn:local;
        destruct (key_end + 4 <=? region_end) eqn:original;
        try reflexivity;
        apply Nat.leb_le in local || apply Nat.leb_gt in local;
        apply Nat.leb_le in original || apply Nat.leb_gt in original;
        lia. }
    rewrite same_guard.
    destruct (key_end + 4 <=? region_end); [|reflexivity].
    cbn [option_map]. unfold rebase_span.
    cbn [Wire.span_number Wire.span_wire Wire.span_start
      Wire.span_key_end Wire.span_payload_start Wire.span_end
      Wire.span_varint_value].
    replace (key_end + 4 - base) with (key_end - base + 4) by lia.
    reflexivity.
Qed.

Lemma framing_field_has_strict_bounded_progress :
  forall bytes finish offset last numeric_order span,
    Wire.framing_field bytes finish offset last numeric_order =
      Some span ->
    Wire.span_start span = offset /\
    offset < Wire.span_end span /\
    Wire.span_end span <= finish.
Proof.
  intros bytes finish offset last numeric_order span framed.
  unfold Wire.framing_field in framed.
  destruct (Wire.canonical_varint bytes finish offset)
    as [[key key_end]|] eqn:key_result; [|discriminate].
  pose proof (canonical_varint_success_bounds bytes finish offset
    key key_end key_result) as [key_progress key_bounded].
  destruct ((0 <? key / 8)%N && Wire.field_number_within_limit
    (key / 8)%N); [|discriminate].
  destruct (negb numeric_order ||
    (last <=? N.to_nat (key / 8)%N)); [|discriminate].
  destruct (N.to_nat (key mod 8)%N)
    as [|[|[|[|[|[|other]]]]]]; try discriminate.
  - destruct (Wire.canonical_varint bytes finish key_end)
      as [[value value_end]|] eqn:value_result; [|discriminate].
    pose proof (canonical_varint_success_bounds bytes finish key_end
      value value_end value_result) as [value_progress value_bounded].
    inversion framed; subst span; cbn.
    lia.
  - destruct (key_end + 8 <=? finish) eqn:bounded; [|discriminate].
    apply Nat.leb_le in bounded.
    inversion framed; subst span; cbn.
    lia.
  - destruct (Wire.canonical_varint bytes finish key_end)
      as [[payload_length body_start]|] eqn:length_result;
      [|discriminate].
    pose proof (canonical_varint_success_bounds bytes finish key_end
      payload_length body_start length_result)
      as [body_progress body_bounded].
    destruct (N.leb payload_length
      (N.of_nat (finish - body_start))) eqn:bounded;
      [|discriminate].
    rewrite Wire.binary_payload_length_guard_exact in bounded by lia.
    apply Nat.leb_le in bounded.
    inversion framed; subst span; cbn.
    lia.
  - destruct (key_end + 4 <=? finish) eqn:bounded; [|discriminate].
    apply Nat.leb_le in bounded.
    inversion framed; subst span; cbn.
    lia.
Qed.

Lemma checked_field_rebase :
  forall bytes base slice_end region_end offset last numeric_order,
    base <= offset /\ offset <= region_end /\
      region_end <= slice_end ->
    Wire.checked_field (region_slice bytes base slice_end)
      (region_end - base) (offset - base) last numeric_order =
    option_map (rebase_span base)
      (Wire.checked_field bytes region_end offset last numeric_order).
Proof.
  intros bytes base slice_end region_end offset last numeric_order
    [before [bounded within_slice]].
  unfold Wire.checked_field.
  rewrite framing_field_rebase by lia.
  destruct (Wire.framing_field bytes region_end offset last numeric_order)
    as [span|] eqn:framed; [|reflexivity].
  cbn [option_map].
  pose proof (framing_field_has_strict_bounded_progress
    bytes region_end offset last numeric_order span framed)
    as [start [progress end_bound]].
  unfold rebase_span.
  cbn [Wire.span_start Wire.span_end].
  rewrite start.
  rewrite !Nat.eqb_refl.
  assert (same_progress :
    (offset - base <? Wire.span_end span - base) =
      (offset <? Wire.span_end span)).
  { destruct (offset - base <? Wire.span_end span - base) eqn:local;
      destruct (offset <? Wire.span_end span) eqn:original;
      try reflexivity;
      apply Nat.ltb_lt in local || apply Nat.ltb_ge in local;
      apply Nat.ltb_lt in original || apply Nat.ltb_ge in original;
      lia. }
  assert (same_bound :
    (Wire.span_end span - base <=? region_end - base) =
      (Wire.span_end span <=? region_end)).
  { destruct (Wire.span_end span - base <=? region_end - base) eqn:local;
      destruct (Wire.span_end span <=? region_end) eqn:original;
      try reflexivity;
      apply Nat.leb_le in local || apply Nat.leb_gt in local;
      apply Nat.leb_le in original || apply Nat.leb_gt in original;
      lia. }
  rewrite same_progress, same_bound.
  cbn [andb].
  destruct ((offset <? Wire.span_end span) &&
    (Wire.span_end span <=? region_end));
    cbn [option_map]; try rewrite start; reflexivity.
Qed.

Lemma reference_remaining_rebase :
  forall fuel bytes base slice_end region_end offset last numeric_order,
    base <= offset /\ offset <= region_end /\
      region_end <= slice_end ->
    Wire.reference_remaining fuel (region_slice bytes base slice_end)
      (region_end - base) (offset - base) last numeric_order =
    Wire.map_parsed (map (rebase_span base))
      (Wire.reference_remaining fuel bytes region_end offset last
        numeric_order).
Proof.
  induction fuel as [|fuel IH];
    intros bytes base slice_end region_end offset last numeric_order
      [before [bounded within_slice]];
    cbn [Wire.reference_remaining].
  all: assert (same_eof :
      (offset - base =? region_end - base) = (offset =? region_end))
      by (destruct (offset - base =? region_end - base) eqn:local;
        destruct (offset =? region_end) eqn:original;
        try reflexivity;
        apply Nat.eqb_eq in local || apply Nat.eqb_neq in local;
        apply Nat.eqb_eq in original || apply Nat.eqb_neq in original;
        lia).
  all: rewrite same_eof.
  all: destruct (offset =? region_end) eqn:eof.
  all: try reflexivity.
  all: rewrite checked_field_rebase by lia.
  all: destruct (Wire.checked_field bytes region_end offset last
      numeric_order) as [span|] eqn:checked.
  all: try reflexivity.
  all: cbn [option_map rebase_span Wire.span_end Wire.span_number].
  pose proof (Wire.checked_field_has_strict_bounded_progress
    bytes region_end offset last numeric_order span checked)
    as [_ [progress end_bound]].
  rewrite (IH bytes base slice_end region_end (Wire.span_end span)
    (Wire.span_number span) numeric_order) by lia.
  destruct (Wire.reference_remaining fuel bytes region_end
    (Wire.span_end span) (Wire.span_number span) numeric_order);
    reflexivity.
Qed.

Theorem worklist_region_rebase :
  forall bytes base slice_end region_start region_end numeric_order,
    base <= region_start /\ region_start <= region_end /\
      region_end <= slice_end /\ slice_end <= length bytes ->
    Wire.worklist_region (region_slice bytes base slice_end)
      (region_start - base) (region_end - base) numeric_order =
    Wire.map_parsed (map (rebase_span base))
      (Wire.worklist_region bytes region_start region_end numeric_order).
Proof.
  intros bytes base slice_end region_start region_end numeric_order bounds.
  rewrite !Wire.worklist_and_reference_agree_on_every_finite_region.
  unfold Wire.reference_region.
  rewrite (region_ok_rebases bytes base slice_end
    region_start region_end bounds).
  destruct (Wire.region_ok bytes region_start region_end);
    [|reflexivity].
  replace (region_end - base - (region_start - base))
    with (region_end - region_start) by lia.
  apply reference_remaining_rebase. lia.
Qed.

Lemma source_descriptor_rebases :
  forall program base span,
    Source.source_descriptor program (rebase_span base span) =
      Source.source_descriptor program span.
Proof.
  intros program base span.
  unfold Source.source_descriptor, Source.wire_kind_of_span,
    rebase_span.
  cbn [Wire.span_number Wire.span_wire].
  reflexivity.
Qed.

Lemma slot_admissible_rebases :
  forall program base current field,
    Source.slot_admissible program (rebase_accumulator base current)
      field =
    Source.slot_admissible program current field.
Proof.
  intros program base current field.
  unfold Source.slot_admissible, rebase_accumulator.
  cbn [Source.decode_last_slot Source.decode_seen_slots].
  reflexivity.
Qed.

Definition child_decoder_rebases
    (base slice_end : nat)
    (absolute_decode local_decode :
      Schema.generated_program -> nat -> nat ->
        option (list Source.source_tree)) : Prop :=
  forall child_program region_start region_end,
    base <= region_start /\ region_start <= region_end /\
      region_end <= slice_end ->
    local_decode child_program (region_start - base)
      (region_end - base) =
    option_map (map (rebase_tree base))
      (absolute_decode child_program region_start region_end).

Lemma generated_source_node_rebases :
  forall base slice_end absolute_decode local_decode span field,
    base <= Wire.span_payload_start span ->
    Wire.span_payload_start span <= Wire.span_end span ->
    Wire.span_end span <= slice_end ->
    child_decoder_rebases base slice_end absolute_decode local_decode ->
    Source.generated_source_node local_decode
      (rebase_span base span) field =
    option_map (rebase_tree base)
      (Source.generated_source_node absolute_decode span field).
Proof.
  intros base slice_end absolute_decode local_decode span field
    before payload_end within children_rebase.
  unfold Source.generated_source_node.
  cbn [rebase_span Wire.span_payload_start Wire.span_end].
  destruct (Schema.descriptor_payload field); try reflexivity.
  destruct (Schema.descriptor_child_type_id field)
    as [child_type|]; [|reflexivity].
  destruct (Schema.lookup_projected_program child_type [] [])
    as [child_program|]; [|reflexivity].
  rewrite (children_rebase child_program
    (Wire.span_payload_start span) (Wire.span_end span)) by lia.
  destruct (absolute_decode child_program
    (Wire.span_payload_start span) (Wire.span_end span));
    reflexivity.
Qed.

Lemma decode_source_step_rebases :
  forall program base slice_end absolute_decode local_decode state span,
    base <= Wire.span_payload_start span ->
    Wire.span_payload_start span <= Wire.span_end span ->
    Wire.span_end span <= slice_end ->
    child_decoder_rebases base slice_end absolute_decode local_decode ->
    Source.decode_source_step program local_decode
      (option_map (rebase_accumulator base) state)
      (rebase_span base span) =
    option_map (rebase_accumulator base)
      (Source.decode_source_step program absolute_decode state span).
Proof.
  intros program base slice_end absolute_decode local_decode
    [current|] span before payload_end within children_rebase;
    [|reflexivity].
  unfold Source.decode_source_step.
  cbn [option_map].
  rewrite source_descriptor_rebases.
  destruct (Source.source_descriptor program span)
    as [field|] eqn:selected; [|reflexivity].
  rewrite slot_admissible_rebases.
  destruct (Source.slot_admissible program current field);
    [|reflexivity].
  rewrite (generated_source_node_rebases base slice_end
    absolute_decode local_decode span field
    before payload_end within children_rebase).
  destruct (Source.generated_source_node absolute_decode span field);
    reflexivity.
Qed.

Lemma decode_source_fold_rebases :
  forall program base slice_end absolute_decode local_decode
      spans state,
    Forall (fun span =>
      base <= Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= slice_end) spans ->
    child_decoder_rebases base slice_end absolute_decode local_decode ->
    fold_left (Source.decode_source_step program local_decode)
      (map (rebase_span base) spans)
      (option_map (rebase_accumulator base) state) =
    option_map (rebase_accumulator base)
      (fold_left (Source.decode_source_step program absolute_decode)
        spans state).
Proof.
  intros program base slice_end absolute_decode local_decode spans.
  induction spans as [|span rest IH]; intros state bounds children_rebase;
    [reflexivity|].
  inversion bounds as [|head tail [before [payload_end within]]
    rest_bounds]; subst head tail.
  cbn [map fold_left].
  rewrite (decode_source_step_rebases program base slice_end
    absolute_decode local_decode state span
    before payload_end within children_rebase).
  apply IH; assumption.
Qed.

(** The local decoder reuses the sliced backing for every nested region.
    [region_end] can be strictly below [slice_end]; this stronger interval
    statement is what the recursive child call requires. *)
Theorem decode_region_rebases :
  forall fuel bytes base slice_end region_start region_end program,
    base <= region_start /\ region_start <= region_end /\
      region_end <= slice_end /\ slice_end <= length bytes ->
    Source.decode_region fuel (region_slice bytes base slice_end) program
      (region_start - base) (region_end - base) =
    option_map (map (rebase_tree base))
      (Source.decode_region fuel bytes program
        region_start region_end).
Proof.
  induction fuel as [|smaller IH];
    intros bytes base slice_end region_start region_end program bounds;
    [reflexivity|].
  cbn [Source.decode_region].
  rewrite worklist_region_rebase by exact bounds.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try reflexivity.
  cbn [Wire.map_parsed].
  pose proof (Source.parsed_region_spans_stay_inside_their_region
    bytes region_start region_end false spans framed) as original_bounds.
  assert (span_bounds : Forall (fun span =>
      base <= Wire.span_payload_start span /\
      Wire.span_payload_start span <= Wire.span_end span /\
      Wire.span_end span <= slice_end) spans).
  { rewrite Forall_forall in original_bounds |- *.
    intros span member.
    destruct (original_bounds span member)
      as [_ [header [payload within]]].
    repeat split; lia. }
  set (absolute_child := fun child_program child_start child_end =>
    Source.decode_region smaller bytes child_program
      child_start child_end).
  set (local_child := fun child_program child_start child_end =>
    Source.decode_region smaller (region_slice bytes base slice_end)
      child_program child_start child_end).
  assert (child_rebase : child_decoder_rebases base slice_end
    absolute_child local_child).
  { unfold child_decoder_rebases, absolute_child, local_child.
    intros child_program child_start child_end child_bounds.
    apply IH. lia. }
  change
    ((option_map (fun state => rev (Source.decode_reversed state))
      (fold_left (Source.decode_source_step program local_child)
        (map (rebase_span base) spans)
        (option_map (rebase_accumulator base)
          (Some (Source.DecodeAccumulator None [] []))))) =
    option_map (map (rebase_tree base))
      (option_map (fun state => rev (Source.decode_reversed state))
        (fold_left (Source.decode_source_step program absolute_child)
          spans (Some (Source.DecodeAccumulator None [] []))))).
  rewrite (decode_source_fold_rebases program base slice_end
    absolute_child local_child spans
    (Some (Source.DecodeAccumulator None [] []))
    span_bounds child_rebase).
  destruct (fold_left (Source.decode_source_step program absolute_child)
    spans (Some (Source.DecodeAccumulator None [] [])))
    as [final|]; cbn [option_map]; [|reflexivity].
  unfold rebase_accumulator.
  cbn [Source.decode_reversed].
  now rewrite map_rev.
Qed.

Print Assumptions region_slice_nth_error.
Print Assumptions region_slice_length.
Print Assumptions region_subslice_rebases.
Print Assumptions region_ok_rebases.
Print Assumptions rebase_span_preserves_number_and_wire.
Print Assumptions source_body_rebases.
Print Assumptions varint_groups_rebase.
Print Assumptions canonical_varint_rebase.
Print Assumptions varint_groups_success_bounds.
Print Assumptions canonical_varint_success_bounds.
Print Assumptions framing_field_rebase.
Print Assumptions framing_field_has_strict_bounded_progress.
Print Assumptions checked_field_rebase.
Print Assumptions reference_remaining_rebase.
Print Assumptions worklist_region_rebase.
Print Assumptions source_descriptor_rebases.
Print Assumptions slot_admissible_rebases.
Print Assumptions generated_source_node_rebases.
Print Assumptions decode_source_step_rebases.
Print Assumptions decode_source_fold_rebases.
Print Assumptions decode_region_rebases.
