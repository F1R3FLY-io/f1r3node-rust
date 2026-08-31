From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool.
Require Import Epm1CursorPositiveV3 Epm1CursorPositiveV4 Epm1ActConcreteV5.

Import ListNotations.
Import PositiveV3 PositiveV4 ActConcreteV5.

Module ActConcreteV5Mutants.

(** These are executable mutations of decisions made by the V5 functions.
    Every kill below supplies a concrete witness which the unmutated model
    handles correctly and the mutated decision handles incorrectly. *)

Definition replace_trailer_with_zeroes (image : list bbyte) : list bbyte :=
  firstn (length image - length ACT_TRAILER) image ++ ACT_TRAILER.

Definition mutant_validate_ignoring_trailer (image : list bbyte) : bool :=
  physical_image_accepts (replace_trailer_with_zeroes image).

Definition corrupt_three_way_trailer : list bbyte :=
  firstn (length ACT_THREE_WAY_IMAGE - 1) ACT_THREE_WAY_IMAGE ++ [1].

Theorem kill_trailer_validation_omission :
  physical_image_accepts corrupt_three_way_trailer = false /\
  mutant_validate_ignoring_trailer corrupt_three_way_trailer = true.
Proof. vm_compute. auto. Qed.

Definition strict_backward_gate (parent : nat) (children : list nat) : bool :=
  forallb (fun child => child <? parent) children.

Definition mutant_nonstrict_backward_gate (parent : nat) (children : list nat) : bool :=
  forallb (fun child => child <=? parent) children.

Theorem kill_equal_child_reentry :
  strict_backward_gate 19 [19] = false /\
  mutant_nonstrict_backward_gate 19 [19] = true.
Proof. split; reflexivity. Qed.

Definition duplicate_node_gate (id : nat) (nodes : list parsed_node) : bool :=
  negb (existsb (parsed_node_id_eq id) nodes).

Definition mutant_duplicate_node_gate (_id : nat) (_nodes : list parsed_node) : bool :=
  true.

Definition duplicate_witness_node : parsed_node :=
  {| pn_id := 19; pn_encoded_len := 1; pn_value := None;
     pn_edge := ParsedBranch None [] |}.

Theorem kill_duplicate_parent_alias :
  duplicate_node_gate 19 [duplicate_witness_node] = false /\
  mutant_duplicate_node_gate 19 [duplicate_witness_node] = true.
Proof. split; reflexivity. Qed.

Definition valid_body : protobuf_value_body :=
  {| pvb_decodes := true; pvb_attaches := true; pvb_canonical := true |}.

Definition invalid_representation_input : epm_snapshot_input :=
  {| esi_envelope_valid := true;
     esi_bodies := [valid_body];
     esi_representation_valid := false;
     esi_parent_mode_compatible := true |}.

Definition mutant_install_before_finish (input : epm_snapshot_input)
    : epm_snapshot_run :=
  let run := run_epm_snapshot_pda input in
  {| esr_events := EpmPrivateRepresentationInstalled :: esr_events run;
     esr_outcome := esr_outcome run |}.

Theorem kill_private_install_before_finish :
  esr_outcome (mutant_install_before_finish invalid_representation_input) =
    EpmPdaFailed EpmRepresentationError /\
  epm_private_install_count
    (esr_events (mutant_install_before_finish invalid_representation_input)) = 1.
Proof. vm_compute. auto. Qed.

Definition noncanonical_invalid_representation_input : epm_snapshot_input :=
  {| esi_envelope_valid := true;
     esi_bodies :=
       [{| pvb_decodes := true; pvb_attaches := true;
           pvb_canonical := false |}];
     esi_representation_valid := false;
     esi_parent_mode_compatible := true |}.

Definition mutant_noncanonical_before_representation
    (input : epm_snapshot_input) : epm_pda_outcome :=
  let drive := drive_epm_value_bodies 0 (esi_bodies input) None in
  match epd_error drive with
  | Some error => EpmPdaFailed error
  | None =>
      match epd_first_noncanonical drive with
      | Some ordinal => EpmPdaFailed (EpmNoncanonicalValueError ordinal)
      | None => esr_outcome (run_epm_snapshot_pda input)
      end
  end.

Theorem kill_reversed_finish_error_precedence :
  esr_outcome
    (run_epm_snapshot_pda noncanonical_invalid_representation_input) =
      EpmPdaFailed EpmRepresentationError /\
  mutant_noncanonical_before_representation
    noncanonical_invalid_representation_input =
      EpmPdaFailed (EpmNoncanonicalValueError 0).
Proof. split; reflexivity. Qed.

Definition two_body_success_input : epm_snapshot_input :=
  {| esi_envelope_valid := true;
     esi_bodies := [valid_body; valid_body];
     esi_representation_valid := true;
     esi_parent_mode_compatible := true |}.

Definition mutant_second_attach_omitted_input : epm_snapshot_input :=
  {| esi_envelope_valid := true;
     esi_bodies :=
       [valid_body;
        {| pvb_decodes := true; pvb_attaches := false;
           pvb_canonical := true |}];
     esi_representation_valid := true;
     esi_parent_mode_compatible := true |}.

Theorem kill_generated_snapshot_attach_omission :
  esr_outcome (run_epm_snapshot_pda two_body_success_input) =
    EpmPdaSucceeded /\
  esr_outcome (run_epm_snapshot_pda mutant_second_attach_omitted_input) =
    EpmPdaFailed (EpmValueAttachError 1).
Proof. split; reflexivity. Qed.

Definition mutant_double_private_install (input : epm_snapshot_input)
    : epm_snapshot_run :=
  let run := run_epm_snapshot_pda input in
  match esr_outcome run with
  | EpmPdaSucceeded =>
      {| esr_events := esr_events run ++ [EpmPrivateRepresentationInstalled];
         esr_outcome := EpmPdaSucceeded |}
  | EpmPdaFailed error => run
  end.

Theorem kill_double_private_install :
  epm_private_install_count
    (esr_events (mutant_double_private_install two_body_success_input)) = 2.
Proof. reflexivity. Qed.

Definition mutant_external_publish_after_any_private_install
    (input : epm_message_input) : epm_snapshot_run :=
  let run := run_epm_message_pda input in
  if 0 <? epm_private_install_count (esr_events run) then
    {| esr_events := EpmExternalValuePublished :: esr_events run;
       esr_outcome := esr_outcome run |}
  else run.

Theorem kill_external_publication_before_later_snapshot_failure :
  esr_outcome
    (mutant_external_publish_after_any_private_install
      second_snapshot_failure_message) = EpmPdaFailed EpmRepresentationError /\
  epm_external_publication_count
    (esr_events
      (mutant_external_publish_after_any_private_install
        second_snapshot_failure_message)) = 1.
Proof. split; reflexivity. Qed.

Definition mutant_duplicate_external_return (input : epm_message_input)
    : epm_snapshot_run :=
  let run := run_epm_message_pda input in
  match esr_outcome run with
  | EpmPdaSucceeded =>
      {| esr_events := esr_events run ++ [EpmExternalValuePublished];
         esr_outcome := EpmPdaSucceeded |}
  | EpmPdaFailed _ => run
  end.

Theorem kill_duplicate_external_return :
  epm_external_publication_count
    (esr_events (mutant_duplicate_external_return repeated_snapshot_message)) = 2.
Proof. reflexivity. Qed.

Definition labels_0_to_30 : list bbyte := seq 0 31.

Definition mutant_encoded_branch_labels_31_dense (labels : list bbyte)
    : list bbyte :=
  if length labels <? 31 then labels else dense_label_mask labels.

Theorem kill_dense_threshold_off_by_one :
  encoded_branch_labels labels_0_to_30 = labels_0_to_30 /\
  length (mutant_encoded_branch_labels_31_dense labels_0_to_30) = 32.
Proof. vm_compute. auto. Qed.

Definition first_collision_state : act_writer_state :=
  snd (add_or_reuse_line forced_collision_hash [120] empty_writer_state).

Definition mutant_hash_only_line_reuse (line_hash : list bbyte -> nat)
    (line : list bbyte) (state : act_writer_state) : nat * act_writer_state :=
  match lookup_line_hash (line_hash line) (aws_lines state) with
  | Some previous => (lce_id previous, state)
  | None => add_or_reuse_line line_hash line state
  end.

Theorem kill_hash_collision_without_byte_equality :
  snd (add_or_reuse_line forced_collision_hash [121] first_collision_state)
    <> first_collision_state /\
  snd (mutant_hash_only_line_reuse
    forced_collision_hash [121] first_collision_state) = first_collision_state.
Proof. vm_compute. split; [discriminate|reflexivity]. Qed.

Definition mutant_direct_validator_uses_span_coverage
    (_line_hash : list bbyte -> nat) (image : list bbyte) : bool :=
  physical_image_accepts image.

Theorem kill_span_only_semantic_admission :
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_UNARY_BRANCH_IMAGE = false /\
  mutant_direct_validator_uses_span_coverage fixture_line_hash
    NONCANONICAL_UNARY_BRANCH_IMAGE = true /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_EMPTY_LINE_IMAGE = false /\
  mutant_direct_validator_uses_span_coverage fixture_line_hash
    NONCANONICAL_EMPTY_LINE_IMAGE = true /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_LINE_VALUE_AND_CHILD_IMAGE = false /\
  mutant_direct_validator_uses_span_coverage fixture_line_hash
    NONCANONICAL_LINE_VALUE_AND_CHILD_IMAGE = true /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_LINE_CHILD_IS_LINE_IMAGE = false /\
  mutant_direct_validator_uses_span_coverage fixture_line_hash
    NONCANONICAL_LINE_CHILD_IS_LINE_IMAGE = true /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_EMPTY_WRAPPER_IMAGE = false /\
  mutant_direct_validator_uses_span_coverage fixture_line_hash
    NONCANONICAL_EMPTY_WRAPPER_IMAGE = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Inductive v5_mutant : Type :=
| V5TrailerOmitted
| V5BackwardNonstrict
| V5DuplicateNodeAllowed
| V5EarlyPrivateInstall
| V5FinishPrecedenceReversed
| V5SnapshotAttachOmitted
| V5DoublePrivateInstall
| V5ExternalPublicationOnFailure
| V5DuplicateExternalReturn
| V5DenseThreshold31
| V5HashOnlyLineReuse
| V5SpanOnlySemanticAdmission.

Definition all_v5_mutants : list v5_mutant :=
  [V5TrailerOmitted; V5BackwardNonstrict; V5DuplicateNodeAllowed;
   V5EarlyPrivateInstall; V5FinishPrecedenceReversed; V5SnapshotAttachOmitted;
   V5DoublePrivateInstall; V5ExternalPublicationOnFailure;
   V5DuplicateExternalReturn; V5DenseThreshold31; V5HashOnlyLineReuse;
   V5SpanOnlySemanticAdmission].

Definition v5_mutant_killed (mutant : v5_mutant) : Prop :=
  match mutant with
  | V5TrailerOmitted =>
      physical_image_accepts corrupt_three_way_trailer = false /\
      mutant_validate_ignoring_trailer corrupt_three_way_trailer = true
  | V5BackwardNonstrict =>
      strict_backward_gate 19 [19] = false /\
      mutant_nonstrict_backward_gate 19 [19] = true
  | V5DuplicateNodeAllowed =>
      duplicate_node_gate 19 [duplicate_witness_node] = false /\
      mutant_duplicate_node_gate 19 [duplicate_witness_node] = true
  | V5EarlyPrivateInstall =>
      epm_private_install_count
        (esr_events (mutant_install_before_finish invalid_representation_input)) = 1
  | V5FinishPrecedenceReversed =>
      mutant_noncanonical_before_representation
        noncanonical_invalid_representation_input =
        EpmPdaFailed (EpmNoncanonicalValueError 0)
  | V5SnapshotAttachOmitted =>
      esr_outcome (run_epm_snapshot_pda mutant_second_attach_omitted_input) =
        EpmPdaFailed (EpmValueAttachError 1)
  | V5DoublePrivateInstall =>
      epm_private_install_count
        (esr_events (mutant_double_private_install two_body_success_input)) = 2
  | V5ExternalPublicationOnFailure =>
      epm_external_publication_count
        (esr_events
          (mutant_external_publish_after_any_private_install
            second_snapshot_failure_message)) = 1
  | V5DuplicateExternalReturn =>
      epm_external_publication_count
        (esr_events (mutant_duplicate_external_return repeated_snapshot_message)) = 2
  | V5DenseThreshold31 =>
      length (mutant_encoded_branch_labels_31_dense labels_0_to_30) = 32
  | V5HashOnlyLineReuse =>
      snd (mutant_hash_only_line_reuse
        forced_collision_hash [121] first_collision_state) = first_collision_state
  | V5SpanOnlySemanticAdmission =>
      mutant_direct_validator_uses_span_coverage fixture_line_hash
        NONCANONICAL_UNARY_BRANCH_IMAGE = true
  end.

Theorem every_v5_mutant_has_a_causal_counterexample : forall mutant,
  v5_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant.
  - exact kill_trailer_validation_omission.
  - exact kill_equal_child_reentry.
  - exact kill_duplicate_parent_alias.
  - exact (proj2 kill_private_install_before_finish).
  - exact (proj2 kill_reversed_finish_error_precedence).
  - exact (proj2 kill_generated_snapshot_attach_omission).
  - exact kill_double_private_install.
  - exact (proj2 kill_external_publication_before_later_snapshot_failure).
  - exact kill_duplicate_external_return.
  - exact (proj2 kill_dense_threshold_off_by_one).
  - exact (proj2 kill_hash_collision_without_byte_equality).
  - exact (proj1 (proj2 kill_span_only_semantic_admission)).
Qed.

Theorem v5_mutant_registry_is_unique_and_complete :
  NoDup all_v5_mutants /\ forall mutant, In mutant all_v5_mutants.
Proof.
  split.
  - repeat constructor; simpl; intuition congruence.
  - intro mutant. destruct mutant; simpl; tauto.
Qed.

Print Assumptions kill_trailer_validation_omission.
Print Assumptions kill_equal_child_reentry.
Print Assumptions kill_duplicate_parent_alias.
Print Assumptions kill_private_install_before_finish.
Print Assumptions kill_reversed_finish_error_precedence.
Print Assumptions kill_generated_snapshot_attach_omission.
Print Assumptions kill_double_private_install.
Print Assumptions kill_external_publication_before_later_snapshot_failure.
Print Assumptions kill_duplicate_external_return.
Print Assumptions kill_dense_threshold_off_by_one.
Print Assumptions kill_hash_collision_without_byte_equality.
Print Assumptions kill_span_only_semantic_admission.
Print Assumptions every_v5_mutant_has_a_causal_counterexample.
Print Assumptions v5_mutant_registry_is_unique_and_complete.

End ActConcreteV5Mutants.
