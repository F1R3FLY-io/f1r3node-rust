From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import RPB3RawRegionTree
  RPB3IndependentOracleRegionRebaseBridge
  RPB3IndependentOracleRecursive.

Module Source := RPB3RawRegionTree.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module Oracle := RPB3IndependentOracleRecursive.

(** A rebase changes only the address of a generated source node.  Under
    checked span bounds, conversion sees the same raw field on either
    backing: tag, wire, body, declared length, and child type all agree. *)
Theorem raw_field_of_source_rebases :
  forall bytes base finish node,
    base <= Source.Wire.span_payload_start (Source.source_span node) ->
    Source.Wire.span_payload_start (Source.source_span node) <=
      Source.Wire.span_end (Source.source_span node) ->
    Source.Wire.span_end (Source.source_span node) <= finish ->
    Source.raw_field_of_source
      (Rebase.region_slice bytes base finish)
      (Rebase.rebase_tree base node) =
    Source.raw_field_of_source bytes node.
Proof.
  intros bytes base finish node before ordered within.
  pose proof (Rebase.source_body_rebases bytes base finish node
    before ordered within) as same_body.
  destruct node as [span field|span field children|span field];
    cbn [Source.source_span] in before, ordered, within;
    unfold Source.raw_field_of_source;
    cbn [Rebase.rebase_tree Source.source_span Source.source_field]
      in same_body |- *;
    rewrite same_body;
    unfold Rebase.rebase_span;
    cbn [Source.Wire.span_number Source.Wire.span_wire
      Source.Wire.span_payload_start Source.Wire.span_end
      Source.wire_kind_of_span];
    replace (Source.Wire.span_end span - base -
      (Source.Wire.span_payload_start span - base))
      with (Source.Wire.span_end span -
        Source.Wire.span_payload_start span) by lia;
    reflexivity.
Qed.

Theorem source_scalar_evaluation_rebases :
  forall recurse hash_fn bytes program base finish span field,
    base <= Source.Wire.span_payload_start span ->
    Source.Wire.span_payload_start span <= Source.Wire.span_end span ->
    Source.Wire.span_end span <= finish ->
    Oracle.source_evaluate_node recurse hash_fn
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base (Source.SourceScalar span field)) =
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceScalar span field).
Proof.
  intros recurse hash_fn bytes program base finish span field
    before ordered within.
  unfold Oracle.source_evaluate_node.
  rewrite (raw_field_of_source_rebases bytes base finish
    (Source.SourceScalar span field)) by
    (cbn [Source.source_span]; lia).
  cbn [Rebase.rebase_tree].
  reflexivity.
Qed.

Theorem source_message_evaluation_rebases :
  forall recurse hash_fn bytes program base finish span field children,
    base <= Source.Wire.span_payload_start span ->
    Source.Wire.span_payload_start span <= Source.Wire.span_end span ->
    Source.Wire.span_end span <= finish ->
    Oracle.source_evaluate_node recurse hash_fn
      (Rebase.region_slice bytes base finish) program
      (Rebase.rebase_tree base
        (Source.SourceMessage span field children)) =
    Oracle.source_evaluate_node recurse hash_fn bytes program
      (Source.SourceMessage span field children).
Proof.
  intros recurse hash_fn bytes program base finish span field children
    before ordered within.
  unfold Oracle.source_evaluate_node.
  rewrite (raw_field_of_source_rebases bytes base finish
    (Source.SourceMessage span field children)) by
    (cbn [Source.source_span]; lia).
  cbn [Rebase.rebase_tree].
  reflexivity.
Qed.

Print Assumptions raw_field_of_source_rebases.
Print Assumptions source_scalar_evaluation_rebases.
Print Assumptions source_message_evaluation_rebases.
