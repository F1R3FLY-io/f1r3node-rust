From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3IndependentOracleRecursive
  RPB3IndependentOracleRegionRebaseBridge
  RPB3IndependentOracleParMessageBodyDecodeBridge
  RPB3IndependentOracleStringMapTypedRebaseBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Oracle := RPB3IndependentOracleRecursive.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module BodyDecode := RPB3IndependentOracleParMessageBodyDecodeBridge.
Module EvalRebase := RPB3IndependentOracleStringMapTypedRebaseBridge.

(** This is the actual selected Par Message child handoff.  Its body is
    freshly decoded by the independent oracle, while the recursive target
    keeps the absolute children stored by the generated source decoder.
    Both the child list and its source evaluations are derived here from
    those two concrete decoders, without a supplied forest alignment. *)
Theorem selected_par_message_child_evaluations_rebase :
  forall bytes nodes span field children raw recurse hash_fn,
    Source.decode_root 1 bytes = Some nodes ->
    In (Source.SourceMessage span field children) nodes ->
    Source.raw_field_of_source bytes
      (Source.SourceMessage span field children) = Some raw ->
    exists type_id child_program,
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] =
        Some child_program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) child_program 0
        (length (Schema.raw_body raw)) =
        Some (map (Rebase.rebase_tree
          (Source.Wire.span_payload_start span)) children) /\
      map (Oracle.source_evaluate_node recurse hash_fn
        (Schema.raw_body raw) child_program)
        (map (Rebase.rebase_tree
          (Source.Wire.span_payload_start span)) children) =
      map (Oracle.source_evaluate_node recurse hash_fn bytes
        child_program) children.
Proof.
  intros bytes nodes span field children raw recurse hash_fn
    decoded member converted.
  destruct (BodyDecode.decoded_par_message_body_has_exact_rebased_children
    bytes nodes span field children raw decoded member converted)
    as [type_id [child_program [raw_type [looked_up local_decoded]]]].
  exists type_id, child_program.
  repeat split; try assumption.
  unfold Source.decode_root in decoded.
  rewrite Source.generated_par_source_lookup_is_present in decoded.
  pose proof (BodyDecode.decoded_region_message_children_from_actual_decoder
    (length bytes) bytes Source.generated_par_source_program
    0 (length bytes) nodes decoded) as provenance.
  rewrite Forall_forall in provenance.
  specialize (provenance _ member).
  cbn [BodyDecode.node_children_from_decoder] in provenance.
  destruct provenance as [actual_type [actual_program
    [family [actual_lookup absolute_decoded]]]].
  pose proof (Source.raw_message_field_uses_generated_child_type
    bytes span field children raw converted) as actual_raw_type.
  rewrite raw_type, family in actual_raw_type.
  inversion actual_raw_type; subst actual_type.
  rewrite looked_up in actual_lookup.
  inversion actual_lookup; subst actual_program.
  pose proof (Source.decoded_region_nodes_have_checked_source_windows
    (length bytes) bytes child_program
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span) children absolute_decoded)
    as [finish_bounded windows].
  assert (checked : Forall (fun node =>
      Source.Wire.span_payload_start span <=
        Source.Wire.span_payload_start (Source.source_span node) /\
      Source.Wire.span_payload_start (Source.source_span node) <=
        Source.Wire.span_end (Source.source_span node) /\
      Source.Wire.span_end (Source.source_span node) <=
        Source.Wire.span_end span) children).
  { rewrite Forall_forall in windows |- *.
    intros node selected.
    unfold Source.source_node_inside_region in windows.
    destruct (windows node selected) as
      [strict [ordered inside]].
    repeat split; lia. }
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceMessage span field children) raw converted)
    as [_ [_ [_ [body _]]]].
  rewrite body.
  change (map (Oracle.source_evaluate_node recurse hash_fn
    (Rebase.region_slice bytes
      (Source.Wire.span_payload_start span)
      (Source.Wire.span_end span)) child_program)
      (map (Rebase.rebase_tree
        (Source.Wire.span_payload_start span)) children) =
    map (Oracle.source_evaluate_node recurse hash_fn bytes
      child_program) children).
  eapply EvalRebase.source_node_evaluations_rebase_pointwise;
    [exact checked|exact finish_bounded].
Qed.

Print Assumptions selected_par_message_child_evaluations_rebase.
