From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3IndependentOracleRecursiveProjection.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.

Definition generated_par_message_families : list (nat * option nat) :=
  [(1, Some 15); (2, Some 19); (4, Some 20); (5, Some 24);
   (6, Some 22); (7, Some 60); (8, Some 55); (11, Some 8);
   (12, Some 23); (13, Some 13)].

(** The finite table is a projection of the generated Par descriptors,
    including their precise tags.  It is not an independent family guess. *)
Lemma generated_par_message_family_table :
  map (fun field =>
    (Schema.descriptor_tag field,
      Schema.descriptor_child_type_id field))
    (filter (fun field =>
      Schema.payload_eqb (Schema.descriptor_payload field)
        Schema.Message)
      (Schema.program_fields Source.generated_par_source_program)) =
    generated_par_message_families.
Proof. vm_compute. reflexivity. Qed.

Lemma selected_par_message_descriptor_family :
  forall span field,
    Source.source_descriptor Source.generated_par_source_program span =
      Some field ->
    Schema.descriptor_payload field = Schema.Message ->
    In (Source.Wire.span_number span,
      Schema.descriptor_child_type_id field)
      generated_par_message_families.
Proof.
  intros span field selected payload.
  pose proof (proj1 (Source.resolved_source_descriptor_has_actual_tag_and_wire
    Source.generated_par_source_program span field selected)) as tag.
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag
    (Schema.program_fields Source.generated_par_source_program)
    (Source.Wire.span_number span)) as [candidate|] eqn:resolved;
    [|discriminate].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span span)
    (Schema.descriptor_wire candidate)); [|discriminate].
  inversion selected; subst candidate.
  pose proof (RecursiveProjection.resolved_generated_field_is_member
    _ _ _ resolved) as member.
  assert (filtered : In field
    (filter (fun descriptor =>
      Schema.payload_eqb (Schema.descriptor_payload descriptor)
        Schema.Message)
      (Schema.program_fields Source.generated_par_source_program))).
  { apply filter_In. split; [exact member|].
    rewrite payload. reflexivity. }
  apply in_map with (f := fun descriptor =>
    (Schema.descriptor_tag descriptor,
      Schema.descriptor_child_type_id descriptor)) in filtered.
  rewrite generated_par_message_family_table in filtered.
  rewrite tag in filtered.
  exact filtered.
Qed.

(** Successful conversion of an actual Message node establishes the Message
    descriptor payload and carries the generated child type into its raw
    field.  The theorem needs no supplied child forest. *)
Theorem selected_par_message_raw_family :
  forall bytes span field nested raw,
    Source.source_descriptor Source.generated_par_source_program span =
      Some field ->
    Source.raw_field_of_source bytes
      (Source.SourceMessage span field nested) = Some raw ->
    In (Schema.raw_tag raw, Schema.raw_program_type_id raw)
      generated_par_message_families.
Proof.
  intros bytes span field nested raw selected converted.
  assert (payload : Schema.descriptor_payload field = Schema.Message).
  { unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    destruct (Schema.descriptor_payload field) eqn:actual;
      cbn [Schema.payload_eqb] in converted;
      try discriminate; reflexivity. }
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    bytes (Source.SourceMessage span field nested) raw converted)
    as [raw_tag _].
  cbn [Source.source_span] in raw_tag.
  pose proof (Source.raw_message_field_uses_generated_child_type
    bytes span field nested raw converted) as raw_type.
  rewrite raw_tag, raw_type.
  exact (selected_par_message_descriptor_family span field
    selected payload).
Qed.

Theorem decoded_par_message_raw_family :
  forall bytes nodes span field nested raw,
    Source.decode_root 1 bytes = Some nodes ->
    In (Source.SourceMessage span field nested) nodes ->
    Source.raw_field_of_source bytes
      (Source.SourceMessage span field nested) = Some raw ->
    In (Schema.raw_tag raw, Schema.raw_program_type_id raw)
      generated_par_message_families.
Proof.
  intros bytes nodes span field nested raw decoded member converted.
  unfold Source.decode_root in decoded.
  rewrite Source.generated_par_source_lookup_is_present in decoded.
  pose proof (Source.decoded_region_nodes_match_generated_source
    (S (length bytes)) bytes Source.generated_par_source_program
    0 (length bytes) nodes decoded) as generated.
  rewrite Forall_forall in generated.
  specialize (generated (Source.SourceMessage span field nested) member).
  destruct generated as [selected _].
  cbn [Source.source_span Source.source_field] in selected.
  exact (selected_par_message_raw_family
    bytes span field nested raw selected converted).
Qed.

Print Assumptions generated_par_message_family_table.
Print Assumptions selected_par_message_descriptor_family.
Print Assumptions selected_par_message_raw_family.
Print Assumptions decoded_par_message_raw_family.
