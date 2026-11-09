From Stdlib Require Import List Arith Lia Bool.
From StackSafePDA Require Import RPB3RawRegionTree
  RPB3IndependentOracleRegionRebaseBridge.

Import ListNotations.

Module Source := RPB3RawRegionTree.
Module Wire := Source.Wire.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.

Definition rebase_entry_pair (base : nat)
    (entry : option Wire.field_span * option Wire.field_span)
    : option Wire.field_span * option Wire.field_span :=
  (option_map (Rebase.rebase_span base) (fst entry),
    option_map (Rebase.rebase_span base) (snd entry)).

Definition rebase_map_entry (base : nat)
    (entry : Source.string_map_entry_source)
    : Source.string_map_entry_source :=
  Source.StringMapEntrySource
    (Rebase.rebase_span base (Source.map_outer_span entry))
    (Source.map_outer_field entry) (Source.map_value_type_id entry)
    (option_map (Rebase.rebase_span base) (Source.map_key_span entry))
    (option_map (Rebase.rebase_span base) (Source.map_value_span entry)).

Lemma map_entry_field_is_rebases :
  forall base tag span,
    Source.map_entry_field_is tag (Rebase.rebase_span base span) =
      Source.map_entry_field_is tag span.
Proof.
  intros base tag span.
  unfold Source.map_entry_field_is, Rebase.rebase_span.
  cbn [Wire.span_number Wire.span_wire].
  reflexivity.
Qed.

Lemma map_span_body_rebases :
  forall bytes base finish span,
    base <= Wire.span_payload_start span ->
    Wire.span_payload_start span <= Wire.span_end span ->
    Wire.span_end span <= finish ->
    Source.map_span_body (Rebase.region_slice bytes base finish)
      (Rebase.rebase_span base span) =
    Source.map_span_body bytes span.
Proof.
  intros bytes base finish span before ordered within.
  unfold Source.map_span_body, Rebase.rebase_span.
  cbn [Wire.span_payload_start Wire.span_end].
  apply Rebase.region_subslice_rebases.
  lia.
Qed.

Lemma parse_string_map_entry_spans_rebases :
  forall base spans,
    Source.parse_string_map_entry_spans
      (map (Rebase.rebase_span base) spans) =
    option_map (rebase_entry_pair base)
      (Source.parse_string_map_entry_spans spans).
Proof.
  intros base spans.
  destruct spans as [|first [|second [|third rest]]];
    cbn [map Source.parse_string_map_entry_spans option_map];
    try reflexivity.
  - rewrite !map_entry_field_is_rebases.
    destruct (Source.map_entry_field_is 1 first);
      [reflexivity|].
    destruct (Source.map_entry_field_is 2 first); reflexivity.
  - rewrite !map_entry_field_is_rebases.
    destruct (Source.map_entry_field_is 1 first &&
      Source.map_entry_field_is 2 second); reflexivity.
Qed.

Theorem decode_string_map_entry_source_rebases :
  forall bytes base finish program span field,
    base <= Wire.span_payload_start span ->
    Wire.span_payload_start span <= Wire.span_end span ->
    Wire.span_end span <= finish ->
    finish <= length bytes ->
    Source.decode_string_map_entry_source
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base (Source.SourceAuxiliary span field)) =
    option_map (rebase_map_entry base)
      (Source.decode_string_map_entry_source bytes program
        (Source.SourceAuxiliary span field)).
Proof.
  intros bytes base finish program span field before ordered within
    finish_bounded.
  unfold Source.decode_string_map_entry_source.
  cbn [Rebase.rebase_tree].
  rewrite Rebase.source_descriptor_rebases.
  destruct (Source.source_descriptor program span)
    as [selected|] eqn:resolved; [|reflexivity].
  destruct (Source.Schema.payload_eqb
    (Source.Schema.descriptor_payload selected)
    Source.Schema.StringMap); [|reflexivity].
  destruct (Source.Schema.descriptor_child_type_id selected)
    as [type_id|]; [|reflexivity].
  cbn [Rebase.rebase_span Wire.span_payload_start Wire.span_end].
  rewrite (Rebase.worklist_region_rebase bytes base finish
    (Wire.span_payload_start span) (Wire.span_end span) true) by lia.
  destruct (Wire.worklist_region bytes
    (Wire.span_payload_start span) (Wire.span_end span) true)
    as [spans| |] eqn:framed; try reflexivity.
  cbn [Wire.map_parsed].
  rewrite parse_string_map_entry_spans_rebases.
  destruct (Source.parse_string_map_entry_spans spans)
    as [[key value]|] eqn:parsed; [|reflexivity].
  cbn [option_map rebase_entry_pair rebase_map_entry fst snd].
  reflexivity.
Qed.

Print Assumptions map_entry_field_is_rebases.
Print Assumptions map_span_body_rebases.
Print Assumptions parse_string_map_entry_spans_rebases.
Print Assumptions decode_string_map_entry_source_rebases.
