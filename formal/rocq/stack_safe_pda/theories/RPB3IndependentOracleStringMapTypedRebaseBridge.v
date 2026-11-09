From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import RPB3RawRegionTree
  RPB3IndependentOracleRegionRebaseBridge
  RPB3IndependentOracleStringMapRegionRebaseBridge
  RPB3IndependentOracleParMessageRawRebaseBridge
  RPB3IndependentOracleRecursive.

Import ListNotations.

Module Source := RPB3RawRegionTree.
Module Wire := Source.Wire.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module EntryRebase := RPB3IndependentOracleStringMapRegionRebaseBridge.
Module RawRebase := RPB3IndependentOracleParMessageRawRebaseBridge.
Module Oracle := RPB3IndependentOracleRecursive.

Definition rebase_typed_entry (base : nat)
    (entry : Source.typed_string_map_entry)
    : Source.typed_string_map_entry :=
  Source.TypedStringMapEntry
    (Rebase.rebase_span base (Source.typed_map_outer_span entry))
    (Source.typed_map_outer_field entry)
    (Source.typed_map_value_type_id entry)
    (Source.typed_map_value_program entry)
    (option_map (fun paired =>
      (Rebase.rebase_span base (fst paired), snd paired))
      (Source.typed_map_key_source entry))
    (option_map (fun paired =>
      (Rebase.rebase_span base (fst paired), snd paired))
      (Source.typed_map_value_source entry)).

Lemma canonical_entry_selected_span_is_member :
  forall spans key value selected,
    Source.canonical_string_map_entry_spans spans key value ->
    key = Some selected \/ value = Some selected ->
    In selected spans.
Proof.
  intros spans key value selected shaped chosen.
  inversion shaped; subst;
    destruct chosen as [chosen|chosen];
    cbn in chosen |- *;
    try discriminate;
    inversion chosen;
    subst; auto.
Qed.

Lemma decoded_string_map_selected_spans_are_bounded :
  forall bytes program outer field entry selected,
    Source.decode_string_map_entry_source bytes program
      (Source.SourceAuxiliary outer field) = Some entry ->
    Source.map_key_span entry = Some selected \/
      Source.map_value_span entry = Some selected ->
    Wire.span_payload_start outer < Wire.span_payload_start selected /\
    Wire.span_payload_start selected <= Wire.span_end selected /\
    Wire.span_end selected <= Wire.span_end outer.
Proof.
  intros bytes program outer field entry selected decoded chosen.
  destruct (Source.decoded_string_map_entry_has_actual_constructor_shape
    bytes program (Source.SourceAuxiliary outer field) entry decoded)
    as [actual_outer [carried [spans
      [node_shape [_ [_ [_ [_ [framed shaped]]]]]]]]].
  inversion node_shape; subst actual_outer carried.
  pose proof (canonical_entry_selected_span_is_member
    spans (Source.map_key_span entry) (Source.map_value_span entry)
    selected shaped chosen) as member.
  pose proof (Source.parsed_region_spans_stay_inside_their_region
    bytes (Wire.span_payload_start outer) (Wire.span_end outer)
    true spans framed) as bounded.
  pose proof (Source.source_forall_member _ _ _ _ bounded member)
    as [_ [strict [ordered within]]].
  repeat split; assumption.
Qed.

Theorem refine_string_map_entry_rebases :
  forall bytes base finish program outer field,
    base <= Wire.span_payload_start outer ->
    Wire.span_payload_start outer <= Wire.span_end outer ->
    Wire.span_end outer <= finish ->
    finish <= length bytes ->
    Source.refine_string_map_entry
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base (Source.SourceAuxiliary outer field)) =
    option_map (rebase_typed_entry base)
      (Source.refine_string_map_entry bytes program
        (Source.SourceAuxiliary outer field)).
Proof.
  intros bytes base finish program outer field before ordered within
    finish_bounded.
  unfold Source.refine_string_map_entry.
  rewrite (EntryRebase.decode_string_map_entry_source_rebases
    bytes base finish program outer field
    before ordered within finish_bounded).
  destruct (Source.decode_string_map_entry_source bytes program
    (Source.SourceAuxiliary outer field)) as [entry|] eqn:decoded;
    [|reflexivity].
  cbn [option_map EntryRebase.rebase_map_entry
    Source.map_value_type_id Source.map_outer_span
    Source.map_outer_field Source.map_key_span Source.map_value_span].
  destruct (Source.Schema.lookup_projected_program
    (Source.map_value_type_id entry) [] []) as [value_program|];
    [|reflexivity].
  destruct (Source.map_key_span entry) as [key|] eqn:key_selected;
    destruct (Source.map_value_span entry) as [value|] eqn:value_selected;
    cbn [option_map rebase_typed_entry fst snd].
  - destruct (decoded_string_map_selected_spans_are_bounded
      bytes program outer field entry key decoded
      (or_introl key_selected)) as [key_before [key_order key_within]].
    destruct (decoded_string_map_selected_spans_are_bounded
      bytes program outer field entry value decoded
      (or_intror value_selected)) as
      [value_before [value_order value_within]].
    rewrite (EntryRebase.map_span_body_rebases bytes base finish key)
      by lia.
    rewrite (EntryRebase.map_span_body_rebases bytes base finish value)
      by lia.
    reflexivity.
  - destruct (decoded_string_map_selected_spans_are_bounded
      bytes program outer field entry key decoded
      (or_introl key_selected)) as [key_before [key_order key_within]].
    rewrite (EntryRebase.map_span_body_rebases bytes base finish key)
      by lia.
    reflexivity.
  - destruct (decoded_string_map_selected_spans_are_bounded
      bytes program outer field entry value decoded
      (or_intror value_selected)) as
      [value_before [value_order value_within]].
    rewrite (EntryRebase.map_span_body_rebases bytes base finish value)
      by lia.
    reflexivity.
  - reflexivity.
Qed.

Theorem source_auxiliary_evaluation_rebases :
  forall recurse hash_fn bytes program base finish outer field,
    base <= Wire.span_payload_start outer ->
    Wire.span_payload_start outer <= Wire.span_end outer ->
    Wire.span_end outer <= finish ->
    finish <= length bytes ->
    Oracle.source_evaluate_node recurse hash_fn
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base (Source.SourceAuxiliary outer field)) =
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceAuxiliary outer field).
Proof.
  intros recurse hash_fn bytes program base finish outer field
    before ordered within finish_bounded.
  unfold Oracle.source_evaluate_node.
  rewrite (RawRebase.raw_field_of_source_rebases bytes base finish
    (Source.SourceAuxiliary outer field)) by
    (cbn [Source.source_span]; lia).
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary outer field)) as [raw|];
    [|reflexivity].
  cbn [Rebase.rebase_tree].
  destruct (Source.Schema.descriptor_payload field);
    try reflexivity.
  fold (Rebase.rebase_tree base (Source.SourceAuxiliary outer field)).
  rewrite (refine_string_map_entry_rebases bytes base finish
    program outer field before ordered within finish_bounded).
  destruct (Source.refine_string_map_entry bytes program
    (Source.SourceAuxiliary outer field)) as [typed|];
    [|reflexivity].
  cbn [option_map rebase_typed_entry Source.typed_map_key_source
    Source.typed_map_value_source].
  destruct (Source.typed_map_key_source typed) as [[key_span key_body]|];
    destruct (Source.typed_map_value_source typed)
      as [[value_span value_body]|];
    reflexivity.
Qed.

Theorem source_node_evaluation_rebases :
  forall recurse hash_fn bytes program base finish node,
    base <= Wire.span_payload_start (Source.source_span node) ->
    Wire.span_payload_start (Source.source_span node) <=
      Wire.span_end (Source.source_span node) ->
    Wire.span_end (Source.source_span node) <= finish ->
    finish <= length bytes ->
    Oracle.source_evaluate_node recurse hash_fn
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base node) =
    Oracle.source_evaluate_node recurse hash_fn bytes program node.
Proof.
  intros recurse hash_fn bytes program base finish
    [span field|span field children|span field]
    before ordered within finish_bounded;
    cbn [Source.source_span] in before, ordered, within.
  - eapply RawRebase.source_scalar_evaluation_rebases;
      eassumption.
  - eapply RawRebase.source_message_evaluation_rebases;
      eassumption.
  - eapply source_auxiliary_evaluation_rebases;
      eassumption.
Qed.

Theorem source_node_evaluations_rebase_pointwise :
  forall recurse hash_fn bytes program base finish nodes,
    Forall (fun node =>
      base <= Wire.span_payload_start (Source.source_span node) /\
      Wire.span_payload_start (Source.source_span node) <=
        Wire.span_end (Source.source_span node) /\
      Wire.span_end (Source.source_span node) <= finish) nodes ->
    finish <= length bytes ->
    map (Oracle.source_evaluate_node recurse hash_fn
      (Rebase.region_slice bytes base finish) program)
      (map (Rebase.rebase_tree base) nodes) =
    map (Oracle.source_evaluate_node recurse hash_fn bytes program)
      nodes.
Proof.
  intros recurse hash_fn bytes program base finish nodes included
    finish_bounded.
  induction included as [|node rest [before [ordered within]]
    included IH]; [reflexivity|].
  cbn [map].
  rewrite (source_node_evaluation_rebases recurse hash_fn bytes
    program base finish node before ordered within finish_bounded).
  now rewrite IH.
Qed.

Print Assumptions canonical_entry_selected_span_is_member.
Print Assumptions decoded_string_map_selected_spans_are_bounded.
Print Assumptions refine_string_map_entry_rebases.
Print Assumptions source_auxiliary_evaluation_rebases.
Print Assumptions source_node_evaluation_rebases.
Print Assumptions source_node_evaluations_rebase_pointwise.
