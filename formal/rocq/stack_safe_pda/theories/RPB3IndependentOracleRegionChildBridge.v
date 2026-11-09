From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3AuxiliaryTypedReference
  RPB3RecursiveAuxiliarySourceReference
  RPB3IndependentOracleRegionRebaseBridge
  RPB3IndependentOracleParMessageBodyDecodeBridge.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module BodyDecode := RPB3IndependentOracleParMessageBodyDecodeBridge.

(** A successful decode passed [region_ok] for its own window.  This fact is
    local to the decoded region, so it also serves nested regions, where the
    root theorems' whole-buffer byte guard is unavailable. *)
Lemma decoded_region_passes_region_guard :
  forall fuel bytes program start finish nodes,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    Source.Wire.region_ok bytes start finish = true.
Proof.
  intros [|smaller] bytes program start finish nodes decoded;
    simpl in decoded; try discriminate.
  destruct (Source.Wire.worklist_region bytes start finish false)
    as [spans| |] eqn:framed; try discriminate.
  unfold Source.Wire.worklist_region in framed.
  destruct (Source.Wire.region_ok bytes start finish) eqn:guard;
    [reflexivity|discriminate].
Qed.

(** A node's payload lies inside the decoded window, so its body inherits the
    byte guard that [region_ok] established for that window. *)
Lemma source_body_inherits_region_byte_guard :
  forall bytes start finish node,
    Source.Wire.region_ok bytes start finish = true ->
    Source.source_node_inside_region start finish node ->
    forallb Source.Wire.byte_ok (Source.source_body bytes node) = true.
Proof.
  intros bytes start finish node guard
    [after_start [payload_end end_region]].
  unfold Source.Wire.region_ok in guard.
  apply andb_true_iff in guard as [_ window_guard].
  rewrite forallb_forall in window_guard |- *.
  intros byte member.
  apply window_guard.
  change (Source.source_body bytes node) with
    (Rebase.region_slice bytes
      (Source.Wire.span_payload_start (Source.source_span node))
      (Source.Wire.span_end (Source.source_span node))) in member.
  rewrite <- (Rebase.region_subslice_rebases bytes start finish
    (Source.Wire.span_payload_start (Source.source_span node))
    (Source.Wire.span_end (Source.source_span node))) in member by lia.
  apply Source.firstn_member_is_original in member.
  apply Source.skipn_member_is_original in member.
  exact member.
Qed.

(** The region-local byte guard is enough for raw-body validity, so a decoded
    node of any region satisfies it without a whole-buffer byte guard. *)
Lemma decoded_node_raw_body_is_valid :
  forall fuel bytes program start finish nodes node raw,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In node nodes ->
    Source.raw_field_of_source bytes node = Some raw ->
    Schema.valid_raw_body raw = true.
Proof.
  intros fuel bytes program start finish nodes node raw
    decoded member converted.
  pose proof (Source.decoded_source_node_has_exact_bounded_body
    fuel bytes program start finish nodes node decoded member)
    as exact_length.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes node raw converted) as [_ [_ [_ [body declared]]]].
  destruct (Source.decoded_region_nodes_have_checked_source_windows
    fuel bytes program start finish nodes decoded) as [_ windows].
  unfold Schema.valid_raw_body.
  rewrite body, declared, exact_length, Nat.eqb_refl.
  exact (source_body_inherits_region_byte_guard bytes start finish node
    (decoded_region_passes_region_guard fuel bytes program start finish
      nodes decoded)
    (Source.source_forall_member _ _ _ _ windows member)).
Qed.

Lemma checked_generated_raw_accepts_matched_valid_node :
  forall bytes program node raw,
    Source.source_descriptor program (Source.source_span node) =
      Some (Source.source_field node) ->
    Source.raw_field_of_source bytes node = Some raw ->
    Schema.valid_raw_body raw = true ->
    Recursive.checked_generated_raw bytes program node = Some raw.
Proof.
  intros bytes program node raw selected converted valid.
  unfold Recursive.checked_generated_raw.
  rewrite selected, converted, valid.
  destruct (Auxiliary.descriptor_eq_dec
    (Source.source_field node) (Source.source_field node))
    as [_|different]; [reflexivity|contradiction].
Qed.

Lemma decoded_node_checked_raw_agrees :
  forall fuel bytes program start finish nodes node,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In node nodes ->
    Recursive.checked_generated_raw bytes program node =
    Source.raw_field_of_source bytes node.
Proof.
  intros fuel bytes program start finish nodes node decoded member.
  destruct (Source.raw_field_of_source bytes node) as [raw|] eqn:converted.
  - destruct (Source.source_forall_member _ _ _ _
      (Source.decoded_region_nodes_match_generated_source
        fuel bytes program start finish nodes decoded) member)
      as [selected _].
    exact (checked_generated_raw_accepts_matched_valid_node
      bytes program node raw selected converted
      (decoded_node_raw_body_is_valid fuel bytes program start finish
        nodes node raw decoded member converted)).
  - destruct (Recursive.checked_generated_raw bytes program node)
      as [raw|] eqn:checked; [|reflexivity].
    destruct (Recursive.checked_generated_raw_preserves_descriptor_body_and_validity
      bytes program node raw checked) as [_ [accepted _]].
    rewrite converted in accepted.
    discriminate accepted.
Qed.

(** Generalizes [decoded_par_message_body_has_exact_rebased_children] from the
    root Par decode to a Message child of an arbitrary decoded region.  The
    stored children were decoded with [fuel], and the premise
    [finish - start <= fuel] puts [S (length body)] within that [fuel], so
    [root_byte_fuel_is_saturated] reaches the fresh decode's own fuel. *)
Theorem decoded_message_body_has_exact_rebased_children :
  forall fuel bytes program start finish nodes span field children raw,
    finish - start <= fuel -> finish <= length bytes ->
    Source.decode_region (S fuel) bytes program start finish = Some nodes ->
    In (Source.SourceMessage span field children) nodes ->
    Source.raw_field_of_source bytes
      (Source.SourceMessage span field children) = Some raw ->
    exists type_id child_program,
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some child_program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) child_program 0 (length (Schema.raw_body raw)) =
      Some (map (Rebase.rebase_tree (Source.Wire.span_payload_start span)) children).
Proof.
  intros fuel bytes program start finish nodes span field children raw
    fuel_covers_region finish_within decoded member converted.
  pose proof (BodyDecode.decoded_region_message_children_from_actual_decoder
    fuel bytes program start finish nodes decoded) as provenance.
  rewrite Forall_forall in provenance.
  specialize (provenance _ member).
  cbn [BodyDecode.node_children_from_decoder] in provenance.
  destruct provenance as [type_id [child_program
    [family [looked_up absolute_decoded]]]].
  pose proof (Source.decoded_region_nodes_have_checked_source_windows
    (S fuel) bytes program start finish nodes decoded) as [_ windows].
  rewrite Forall_forall in windows.
  specialize (windows _ member).
  unfold Source.source_node_inside_region in windows.
  cbn [Source.source_span] in windows.
  destruct windows as [after_start [payload_end end_region]].
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceMessage span field children) raw converted)
    as [_ [_ [_ [body _]]]].
  pose proof (Source.raw_message_field_uses_generated_child_type
    bytes span field children raw converted) as raw_type.
  rewrite family in raw_type.
  exists type_id, child_program.
  split; [exact raw_type|].
  split; [exact looked_up|].
  assert (body_length : length (Schema.raw_body raw) =
    Source.Wire.span_end span -
      Source.Wire.span_payload_start span).
  { rewrite body.
    eapply Source.bounded_source_body_has_exact_length;
      cbn [Source.source_span]; lia. }
  pose proof (Rebase.decode_region_rebases fuel bytes
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span)
    (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span) child_program) as rebased.
  specialize (rebased ltac:(lia)).
  rewrite absolute_decoded in rebased.
  cbn [option_map] in rebased.
  rewrite Nat.sub_diag in rebased.
  change (Source.decode_region fuel
    (Source.source_body bytes
      (Source.SourceMessage span field children)) child_program
    0 (Source.Wire.span_end span -
      Source.Wire.span_payload_start span) =
    Some (map (Rebase.rebase_tree
      (Source.Wire.span_payload_start span)) children)) in rebased.
  rewrite <- body in rebased.
  rewrite <- body_length in rebased.
  replace fuel with
    (S (length (Schema.raw_body raw)) +
      (fuel - S (length (Schema.raw_body raw)))) in rebased
    by lia.
  rewrite Source.root_byte_fuel_is_saturated in rebased.
  exact rebased.
Qed.

Print Assumptions decoded_region_passes_region_guard.
Print Assumptions source_body_inherits_region_byte_guard.
Print Assumptions decoded_node_raw_body_is_valid.
Print Assumptions checked_generated_raw_accepts_matched_valid_node.
Print Assumptions decoded_node_checked_raw_agrees.
Print Assumptions decoded_message_body_has_exact_rebased_children.
