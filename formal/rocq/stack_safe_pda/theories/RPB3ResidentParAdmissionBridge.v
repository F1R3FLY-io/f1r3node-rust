From Stdlib Require Import List Bool Arith String.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3DescriptorLaws RPB3GeneratedFieldRefinement
  RPB3SourceReferenceIntegration.

Import ListNotations.
Import GeneratedData.
Open Scope string_scope.

Definition shared_par_scalar_tag (tag : nat) : bool :=
  Nat.eqb tag 9 || Nat.eqb tag 10.

Definition native_slot_signature (slot : native_slot)
    : nat * string * field_kind * bool * default_policy * option nat :=
  (native_slot_tag slot, native_slot_name slot, native_slot_kind slot,
    native_slot_repeated slot, native_slot_default slot,
    native_slot_child_id slot).

Definition raw_slot_signature (slot : projection_slot)
    : nat * string * field_kind * bool * default_policy * option nat :=
  (slot_min_tag slot, slot_name slot, slot_field_kind slot,
    slot_repeated slot, slot_default slot, slot_child_id slot).

Definition native_shared_scalar_signatures :=
  map native_slot_signature
    (filter (fun slot => shared_par_scalar_tag (native_slot_tag slot))
      (native_program_slots resident_par_program)).

Definition raw_shared_scalar_signatures :=
  map raw_slot_signature
    (filter (fun slot => shared_par_scalar_tag (slot_min_tag slot))
      (program_slots raw_par_program)).

Definition raw_component_slots :=
  filter (fun slot => negb (shared_par_scalar_tag (slot_min_tag slot)))
    (program_slots raw_par_program).

Definition raw_component_signature (slot : projection_slot)
    : nat * string * option nat :=
  (slot_min_tag slot, slot_name slot, slot_child_id slot).

Definition raw_component_signatures :=
  map raw_component_signature raw_component_slots.

Definition raw_component_wire_shape : bool :=
  forallb (fun slot =>
    match slot_field_kind slot, slot_protobuf_kind slot, slot_wire slot,
          slot_default slot, slot_arms slot with
    | FKMessageSequence, PBRepeatedMessage, Some WTLengthDelimited,
      DefaultNotScalar, [] => slot_repeated slot
    | _, _, _, _, _ => false
    end) raw_component_slots.

Definition native_par_raw_relation : Prop :=
  native_program_type_id resident_par_program =
    program_type_id raw_par_program /\
  native_program_name resident_par_program =
    program_name raw_par_program /\
  native_program_protobuf_field_count resident_par_program = 0 /\
  map native_slot_index (native_program_slots resident_par_program) =
    [0; 1; 2] /\
  map native_slot_tag (native_program_slots resident_par_program) =
    [0; 9; 10] /\
  nth_error (native_program_slots resident_par_program) 0 =
    Some {| native_slot_index := 0; native_slot_tag := 0;
      native_slot_name := "components"; native_slot_kind := FKParBag;
      native_slot_repeated := false;
      native_slot_default := DefaultNotScalar;
      native_slot_child_id := None |} /\
  native_shared_scalar_signatures = raw_shared_scalar_signatures /\
  raw_component_signatures =
    [(1, "sends", Some 15); (2, "receives", Some 19);
     (4, "news", Some 20); (5, "exprs", Some 24);
     (6, "matches", Some 22); (7, "unforgeables", Some 60);
     (8, "connectives", Some 55); (11, "bundles", Some 8);
     (12, "conditionals", Some 23); (13, "cost_token_stacks", Some 13)] /\
  raw_component_wire_shape = true /\
  nth_error generated_programs 0 = Some raw_par_program.

Theorem native_par_raw_relation_exact : native_par_raw_relation.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem accepted_source_par_uses_live_native_identity_and_raw_wire_program :
  forall body semantic_default summary,
    RPB3SourceReferenceIntegration.source_reference_par_of_body body =
      Some (semantic_default, summary) ->
    exists program root,
      RPB3GeneratedFieldRefinement.lookup_projected_program
        (native_program_type_id resident_par_program) [] [] =
        Some program /\
      RPB3GeneratedFieldRefinement.program_family program =
        native_program_type_id resident_par_program /\
      native_shared_scalar_signatures = raw_shared_scalar_signatures /\
      raw_component_signatures =
        [(1, "sends", Some 15); (2, "receives", Some 19);
         (4, "news", Some 20); (5, "exprs", Some 24);
         (6, "matches", Some 22); (7, "unforgeables", Some 60);
         (8, "connectives", Some 55); (11, "bundles", Some 8);
         (12, "conditionals", Some 23); (13, "cost_token_stacks", Some 13)] /\
      raw_component_wire_shape = true /\
      RPB3GeneratedFieldRefinement.worklist_outcome root =
        RPB3GeneratedFieldRefinement.AcceptedSummary summary.
Proof.
  intros body semantic_default summary accepted.
  destruct (RPB3SourceReferenceIntegration.source_reference_par_body_has_decoded_reference
      body semantic_default summary accepted)
    as [program [nodes [root facts]]].
  destruct facts as [looked_up
    [_ [_ [_ [_ [_ [_ [_ worklist]]]]]]]].
  destruct native_par_raw_relation_exact as
    [same_type [_ [_ [_ [_ [_ [shared_scalars
      [component_sigs [component_shape _]]]]]]]]].
  exists program, root.
  split.
  - rewrite same_type. exact looked_up.
  - split.
    + assert (raw_lookup :
        RPB3GeneratedFieldRefinement.lookup_projected_program 1 [] [] =
          Some RPB3GeneratedFieldRefinement.raw_par_program).
      { vm_compute. reflexivity. }
      rewrite raw_lookup in looked_up.
      inversion looked_up; subst program.
      rewrite same_type.
      vm_compute. reflexivity.
    + split.
      * exact shared_scalars.
      * split.
        -- exact component_sigs.
        -- split.
           ++ exact component_shape.
           ++ exact worklist.
Qed.

Print Assumptions native_par_raw_relation_exact.
Print Assumptions accepted_source_par_uses_live_native_identity_and_raw_wire_program.
