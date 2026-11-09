From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3IndependentOracleRegionRebaseBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.

(** A generated Message node stores precisely the children decoded while
    constructing that node.  This predicate records the provenance of those
    children without assuming an alignment with a later fresh decode. *)
Definition node_children_from_decoder
    (decode_child : Schema.generated_program -> nat -> nat ->
      option (list Source.source_tree))
    (node : Source.source_tree) : Prop :=
  match node with
  | Source.SourceMessage span field children =>
      exists type_id child_program,
        Schema.descriptor_child_type_id field = Some type_id /\
        Schema.lookup_projected_program type_id [] [] =
          Some child_program /\
        decode_child child_program
          (Source.Wire.span_payload_start span)
          (Source.Wire.span_end span) = Some children
  | Source.SourceScalar _ _ | Source.SourceAuxiliary _ _ => True
  end.

Lemma generated_node_children_from_decoder :
  forall decode_child span field node,
    Source.generated_source_node decode_child span field = Some node ->
    node_children_from_decoder decode_child node.
Proof.
  intros decode_child span field node generated.
  unfold Source.generated_source_node in generated.
  destruct (Schema.descriptor_payload field) eqn:payload;
    try (inversion generated; exact I).
  destruct (Schema.descriptor_child_type_id field)
    as [type_id|] eqn:family; [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [child_program|] eqn:looked_up; [|discriminate].
  destruct (decode_child child_program
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span)) as [children|] eqn:decoded;
    [|discriminate].
  inversion generated; subst node.
  unfold node_children_from_decoder.
  exists type_id, child_program.
  repeat split; assumption.
Qed.

Lemma decode_step_preserves_child_provenance :
  forall program decode_child current span next,
    Source.decode_source_step program decode_child (Some current) span =
      Some next ->
    Forall (node_children_from_decoder decode_child)
      (Source.decode_reversed current) ->
    Forall (node_children_from_decoder decode_child)
      (Source.decode_reversed next).
Proof.
  intros program decode_child current span next stepped previous.
  unfold Source.decode_source_step in stepped.
  destruct (Source.source_descriptor program span)
    as [field|] eqn:selected; [|discriminate].
  destruct (Source.slot_admissible program current field);
    [|discriminate].
  destruct (Source.generated_source_node decode_child span field)
    as [node|] eqn:generated; [|discriminate].
  inversion stepped; subst next.
  constructor; [|exact previous].
  eapply generated_node_children_from_decoder; exact generated.
Qed.

Theorem decoded_region_message_children_from_actual_decoder :
  forall fuel bytes program region_start region_end nodes,
    Source.decode_region (S fuel) bytes program region_start region_end =
      Some nodes ->
    Forall (node_children_from_decoder
      (fun child_program start finish =>
        Source.decode_region fuel bytes child_program start finish)) nodes.
Proof.
  intros fuel bytes program region_start region_end nodes decoded.
  cbn [Source.decode_region] in decoded.
  destruct (Source.Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate.
  set (decode_child := fun child_program start finish =>
    Source.decode_region fuel bytes child_program start finish).
  change (option_map (fun state => rev (Source.decode_reversed state))
    (fold_left (Source.decode_source_step program decode_child) spans
      (Some (Source.DecodeAccumulator None [] []))) = Some nodes)
    in decoded.
  destruct (fold_left (Source.decode_source_step program decode_child) spans
    (Some (Source.DecodeAccumulator None [] []))) as [final|]
    eqn:folded; [|discriminate].
  inversion decoded; subst nodes.
  apply Source.source_forall_reverses.
  eapply Source.successful_option_fold_preserves_invariant with
    (step := Source.decode_source_step program decode_child)
    (invariant := fun current =>
      Forall (node_children_from_decoder decode_child)
        (Source.decode_reversed current))
    (initial := Source.DecodeAccumulator None [] []);
    [| |constructor|exact folded].
  - intro member. reflexivity.
  - intros current member next advanced previous.
    eapply decode_step_preserves_child_provenance;
      [exact advanced|exact previous].
Qed.

(** This is the actual Par Message handoff: the stored absolute child forest
    rebases to the fresh decoder on the converted raw body.  The child program
    and type ID are obtained from the generated descriptor, and the fuel
    difference is eliminated by the byte-depth stability theorem. *)
Theorem decoded_par_message_body_has_exact_rebased_children :
  forall bytes nodes span field children raw,
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
            (Source.Wire.span_payload_start span)) children).
Proof.
  intros bytes nodes span field children raw decoded member converted.
  unfold Source.decode_root in decoded.
  rewrite Source.generated_par_source_lookup_is_present in decoded.
  pose proof (decoded_region_message_children_from_actual_decoder
    (length bytes) bytes Source.generated_par_source_program
    0 (length bytes) nodes decoded) as provenance.
  rewrite Forall_forall in provenance.
  specialize (provenance _ member).
  cbn [node_children_from_decoder] in provenance.
  destruct provenance as [type_id [child_program
    [family [looked_up absolute_decoded]]]].
  pose proof (Source.decoded_region_nodes_have_checked_source_windows
    (S (length bytes)) bytes Source.generated_par_source_program
    0 (length bytes) nodes decoded) as [_ windows].
  rewrite Forall_forall in windows.
  specialize (windows _ member).
  unfold Source.source_node_inside_region in windows.
  cbn [Source.source_span] in windows.
  destruct windows as [positive [ordered bounded]].
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceMessage span field children) raw converted)
    as [_ [_ [_ [body _]]]].
  pose proof (Source.raw_message_field_uses_generated_child_type
    bytes span field children raw converted) as raw_type.
  rewrite family in raw_type.
  exists type_id, child_program.
  split; [exact raw_type|].
  split; [exact looked_up|].
  pose proof (Rebase.decode_region_rebases (length bytes) bytes
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span)
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span) child_program) as rebased.
  specialize (rebased ltac:(lia)).
  rewrite absolute_decoded in rebased.
  cbn [option_map] in rebased.
  rewrite Nat.sub_diag in rebased.
  change (Source.decode_region (length bytes)
    (Source.source_body bytes
      (Source.SourceMessage span field children)) child_program
    0 (Source.Wire.span_end span -
      Source.Wire.span_payload_start span) =
    Some (map (Rebase.rebase_tree
      (Source.Wire.span_payload_start span)) children)) in rebased.
  rewrite <- body in rebased.
  assert (body_length : length (Schema.raw_body raw) =
    Source.Wire.span_end span -
      Source.Wire.span_payload_start span).
  { rewrite body.
    eapply Source.bounded_source_body_has_exact_length;
      cbn [Source.source_span]; lia. }
  rewrite <- body_length in rebased.
  replace (length bytes) with
    (S (length (Schema.raw_body raw)) +
      (length bytes - S (length (Schema.raw_body raw)))) in rebased
    by lia.
  rewrite Source.root_byte_fuel_is_saturated in rebased.
  exact rebased.
Qed.

Print Assumptions generated_node_children_from_decoder.
Print Assumptions decode_step_preserves_child_provenance.
Print Assumptions decoded_region_message_children_from_actual_decoder.
Print Assumptions decoded_par_message_body_has_exact_rebased_children.
