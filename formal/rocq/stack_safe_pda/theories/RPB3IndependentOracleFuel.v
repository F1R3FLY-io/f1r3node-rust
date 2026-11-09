From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import EPM1 EPM1ACTree03 RPB3WireCursor
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3RecursiveAuxiliarySourceReference RPB3IndependentOracleRecursive.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Wire := RPB3WireCursor.
Module Bridge := RPB3RecursiveAuxiliarySourceReference.
Module Oracle := RPB3IndependentOracleRecursive.

(** The root source decoder checks every field against the physical region.
    Each protobuf field consumes at least one header byte, so its body is
    strictly shorter than the containing message body. *)
Lemma decoded_source_node_body_is_strictly_smaller :
  forall fuel body program nodes node,
    Source.decode_region fuel body program 0 (length body) = Some nodes ->
    In node nodes ->
    length (Source.source_body body node) < length body.
Proof.
  intros fuel body program nodes node decoded member.
  destruct (Source.decoded_region_nodes_have_checked_source_windows
    fuel body program 0 (length body) nodes decoded)
    as [_ bounded].
  rewrite Forall_forall in bounded.
  specialize (bounded node member).
  unfold Source.source_node_inside_region in bounded.
  destruct bounded as [header [payload_end end_within]].
  pose proof (Source.decoded_source_node_has_exact_bounded_body
    fuel body program 0 (length body) nodes node decoded member)
    as exact_length.
  rewrite exact_length. lia.
Qed.

Lemma decoded_source_raw_child_body_is_strictly_smaller :
  forall fuel body program nodes node raw,
    Source.decode_region fuel body program 0 (length body) = Some nodes ->
    In node nodes ->
    Source.raw_field_of_source body node = Some raw ->
    length (Schema.raw_body raw) < length body.
Proof.
  intros fuel body program nodes node raw decoded member converted.
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    body node raw converted) as [_ [_ [_ [same_body _]]]].
  rewrite same_body.
  eapply decoded_source_node_body_is_strictly_smaller;
    [exact decoded|exact member].
Qed.

Lemma refined_string_map_par_body_is_strictly_smaller :
  forall body program outer_span outer_field typed value_span value,
    Source.refine_string_map_entry body program
      (Source.SourceAuxiliary outer_span outer_field) = Some typed ->
    Source.typed_map_value_source typed = Some (value_span, value) ->
    length value < length body.
Proof.
  intros body program outer_span outer_field typed value_span value
    refined selected.
  destruct (Bridge.refined_string_map_value_is_strictly_inside_parent_bytes
    body program outer_span outer_field typed value_span value
    refined selected) as [_ [_ [_ strict]]].
  exact strict.
Qed.

Lemma decoded_pathmap_par_body_is_strictly_smaller :
  forall body node raw hash_fn snapshot trie value,
    Source.raw_field_of_source body node = Some raw ->
    EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Schema.raw_body raw) = Some (snapshot, trie) ->
    In value (EPM1.value_bodies snapshot) ->
    length value < length body.
Proof.
  intros body node raw hash_fn snapshot trie value converted
    decoded member.
  pose proof (EPM1ACTree03.decode_epm1_actree03_refines_epm1
    hash_fn (Schema.raw_body raw) snapshot trie decoded) as epm.
  pose proof (Bridge.decoded_epm1_value_bodies_are_strictly_shorter_than_snapshot
    (Schema.raw_body raw) snapshot epm) as strict_values.
  rewrite Forall_forall in strict_values.
  specialize (strict_values value member).
  destruct (Source.raw_field_of_source_preserves_wire_and_body
    body node raw converted) as [_ [_ [_ [same_body _]]]].
  rewrite same_body in strict_values.
  pose proof (Bridge.source_body_length_at_most_parent_bytes body node)
    as bounded.
  eapply Nat.lt_le_trans; [exact strict_values|exact bounded].
Qed.

Definition source_fuel_covers_body (fuel : nat)
    (body : list nat) : Prop :=
  S (length body) <= fuel.

Lemma strictly_smaller_body_preserves_source_fuel :
  forall fuel parent child,
    source_fuel_covers_body fuel parent ->
    length child < length parent ->
    source_fuel_covers_body (Nat.pred fuel) child.
Proof.
  intros [|fuel] parent child enough strict;
    unfold source_fuel_covers_body in *; simpl in *; lia.
Qed.

Theorem decoded_message_child_has_adequate_recursive_fuel :
  forall fuel body program nodes node raw,
    source_fuel_covers_body fuel body ->
    Source.decode_region (S (length body)) body program 0
      (length body) = Some nodes ->
    In node nodes ->
    Source.raw_field_of_source body node = Some raw ->
    source_fuel_covers_body (Nat.pred fuel) (Schema.raw_body raw).
Proof.
  intros fuel body program nodes node raw enough decoded member converted.
  eapply strictly_smaller_body_preserves_source_fuel;
    [exact enough|].
  eapply decoded_source_raw_child_body_is_strictly_smaller;
    eauto.
Qed.

Theorem refined_string_map_child_has_adequate_recursive_fuel :
  forall fuel body program outer_span outer_field typed value_span value,
    source_fuel_covers_body fuel body ->
    Source.refine_string_map_entry body program
      (Source.SourceAuxiliary outer_span outer_field) = Some typed ->
    Source.typed_map_value_source typed = Some (value_span, value) ->
    source_fuel_covers_body (Nat.pred fuel) value.
Proof.
  intros fuel body program outer_span outer_field typed value_span value
    enough refined selected.
  eapply strictly_smaller_body_preserves_source_fuel;
    [exact enough|].
  eapply refined_string_map_par_body_is_strictly_smaller;
    eauto.
Qed.

Theorem decoded_pathmap_child_has_adequate_recursive_fuel :
  forall fuel body node raw hash_fn snapshot trie value,
    source_fuel_covers_body fuel body ->
    Source.raw_field_of_source body node = Some raw ->
    EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Schema.raw_body raw) = Some (snapshot, trie) ->
    In value (EPM1.value_bodies snapshot) ->
    source_fuel_covers_body (Nat.pred fuel) value.
Proof.
  intros fuel body node raw hash_fn snapshot trie value
    enough converted decoded member.
  eapply strictly_smaller_body_preserves_source_fuel;
    [exact enough|].
  eapply decoded_pathmap_par_body_is_strictly_smaller;
    eauto.
Qed.

Lemma source_root_starts_with_adequate_fuel :
  forall body,
    source_fuel_covers_body (S (length body)) body.
Proof.
  intros body. unfold source_fuel_covers_body. lia.
Qed.

Print Assumptions decoded_source_node_body_is_strictly_smaller.
Print Assumptions decoded_source_raw_child_body_is_strictly_smaller.
Print Assumptions refined_string_map_par_body_is_strictly_smaller.
Print Assumptions decoded_pathmap_par_body_is_strictly_smaller.
Print Assumptions strictly_smaller_body_preserves_source_fuel.
Print Assumptions decoded_message_child_has_adequate_recursive_fuel.
Print Assumptions refined_string_map_child_has_adequate_recursive_fuel.
Print Assumptions decoded_pathmap_child_has_adequate_recursive_fuel.
Print Assumptions source_root_starts_with_adequate_fuel.
