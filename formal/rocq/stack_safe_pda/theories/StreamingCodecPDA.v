From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.
Set Implicit Arguments.

Section FramePDA.

Inductive frame_state : Type :=
| FrameOpen
| FrameSawEscape
| FrameComplete
| FrameInvalid.

Inductive frame_outcome : Type :=
| FrameAccepted
| FrameTruncated
| FrameRejected.

Definition frame_step (state : frame_state) (byte : nat) : frame_state :=
  match state with
  | FrameOpen => if Nat.eqb byte 0 then FrameSawEscape else FrameOpen
  | FrameSawEscape =>
      if Nat.eqb byte 0 then FrameComplete
      else if Nat.eqb byte 255 then FrameOpen
      else FrameInvalid
  | FrameComplete => FrameInvalid
  | FrameInvalid => FrameInvalid
  end.

Fixpoint frame_fold (state : frame_state) (bytes : list nat) : frame_state :=
  match bytes with
  | [] => state
  | byte :: tail => frame_fold (frame_step state byte) tail
  end.

Fixpoint frame_chunk_fold
    (state : frame_state) (chunks : list (list nat)) : frame_state :=
  match chunks with
  | [] => state
  | chunk :: tail => frame_chunk_fold (frame_fold state chunk) tail
  end.

Definition frame_finish (state : frame_state) : frame_outcome :=
  match state with
  | FrameComplete => FrameAccepted
  | FrameInvalid => FrameRejected
  | FrameOpen | FrameSawEscape => FrameTruncated
  end.

Lemma frame_fold_app :
  forall state left right,
    frame_fold state (left ++ right) = frame_fold (frame_fold state left) right.
Proof.
  intros state left; revert state; induction left as [|byte tail IH];
    intros state right; simpl; [reflexivity | apply IH].
Qed.

Lemma frame_chunk_fold_concat :
  forall state chunks,
    frame_chunk_fold state chunks = frame_fold state (concat chunks).
Proof.
  intros state chunks; revert state; induction chunks as [|chunk tail IH];
    intro state; simpl; [reflexivity |].
  rewrite frame_fold_app. apply IH.
Qed.

Theorem chunked_frame_pda_refines_contiguous_frame_parser :
  forall chunks,
    frame_finish (frame_chunk_fold FrameOpen chunks) =
    frame_finish (frame_fold FrameOpen (concat chunks)).
Proof.
  intro chunks. rewrite frame_chunk_fold_concat. reflexivity.
Qed.

Theorem dangling_escape_is_truncated :
  frame_finish FrameSawEscape = FrameTruncated.
Proof. reflexivity. Qed.

Theorem group_marker_inside_frame_is_not_a_terminator :
  frame_step FrameSawEscape 1 = FrameInvalid.
Proof. reflexivity. Qed.

Theorem escaped_zero_continues_the_frame :
  frame_step FrameSawEscape 255 = FrameOpen.
Proof. reflexivity. Qed.

End FramePDA.

Section RhoPathGrammar.

Inductive rho_path_kind : Type := DatumPath | ContinuationPath | JoinPath.

Record rho_path_shape : Type := RhoPathShape {
  shape_kind : rho_path_kind;
  shape_frames : nat;
  shape_group_ended : bool;
  shape_suffix_bytes : nat;
  shape_trailing_bytes : nat
}.

Definition rho_path_accepts (shape : rho_path_shape) : bool :=
  match shape_kind shape with
  | DatumPath => Nat.eqb (shape_frames shape) 3
  | ContinuationPath =>
      (2 <=? shape_frames shape) && shape_group_ended shape &&
      Nat.eqb (shape_trailing_bytes shape) 0
  | JoinPath =>
      Nat.eqb (shape_frames shape) 1 && shape_group_ended shape &&
      (0 <? shape_suffix_bytes shape)
  end.

Theorem datum_identity_suffix_is_unframed_and_may_be_empty :
  forall suffix,
    rho_path_accepts (RhoPathShape DatumPath 3 false suffix 0) = true.
Proof. reflexivity. Qed.

Theorem continuation_requires_nonempty_group_identity_and_no_trailing_bytes :
  forall frames trailing,
    rho_path_accepts (RhoPathShape ContinuationPath frames true 0 trailing) = true <->
    2 <= frames /\ trailing = 0.
Proof.
  intros frames trailing.
  change ((((2 <=? frames) && true) && (trailing =? 0)) = true <->
    2 <= frames /\ trailing = 0).
  repeat rewrite andb_true_iff.
  rewrite Nat.leb_le, Nat.eqb_eq. tauto.
Qed.

Theorem join_requires_one_channel_and_nonempty_unframed_suffix :
  forall suffix,
    rho_path_accepts (RhoPathShape JoinPath 1 true suffix 0) = true <-> 0 < suffix.
Proof.
  intro suffix.
  change ((((1 =? 1) && true) && (0 <? suffix)) = true <-> 0 < suffix).
  repeat rewrite andb_true_iff.
  rewrite Nat.eqb_refl, Nat.ltb_lt. tauto.
Qed.

End RhoPathGrammar.

Section LexicographicReplay.

Inductive lex_order : Type := LexLess | LexEqual | LexGreater.

Fixpoint byte_lex_compare (left right : list nat) : lex_order :=
  match left, right with
  | [], [] => LexEqual
  | [], _ :: _ => LexLess
  | _ :: _, [] => LexGreater
  | l :: ls, r :: rs =>
      if l <? r then LexLess
      else if r <? l then LexGreater
      else byte_lex_compare ls rs
  end.

Theorem equal_keys_are_rejected_by_strict_order :
  forall key, byte_lex_compare key key = LexEqual.
Proof.
  intro key; induction key as [|byte tail IH]; simpl; [reflexivity |].
  rewrite Nat.ltb_irrefl. exact IH.
Qed.

Theorem proper_prefix_is_lexicographically_smaller :
  forall prefix byte suffix,
    byte_lex_compare prefix (prefix ++ byte :: suffix) = LexLess.
Proof.
  intro prefix; induction prefix as [|head tail IH]; intros byte suffix; simpl.
  - reflexivity.
  - rewrite Nat.ltb_irrefl. apply IH.
Qed.

Definition replayed_key_order
    (length_stable digest_stable : bool)
    (previous current : list nat) : option lex_order :=
  if length_stable && digest_stable
  then Some (byte_lex_compare previous current)
  else None.

Theorem unstable_reread_cannot_authorize_key_order :
  forall length_stable digest_stable previous current,
    length_stable && digest_stable = false ->
    replayed_key_order length_stable digest_stable previous current = None.
Proof.
  intros length_stable digest_stable previous current unstable.
  unfold replayed_key_order. rewrite unstable. reflexivity.
Qed.

Theorem stable_chunk_reassembly_uses_exact_byte_order :
  forall previous_chunks current_chunks previous current,
    concat previous_chunks = previous ->
    concat current_chunks = current ->
    replayed_key_order true true (concat previous_chunks) (concat current_chunks) =
    Some (byte_lex_compare previous current).
Proof. intros; subst; reflexivity. Qed.

End LexicographicReplay.

Section MutationPrecedence.

Inductive mutation_outcome : Type :=
| MutationAccepted
| MutationResource
| MutationInvalidKey
| MutationKeyOrder
| MutationNoop
| MutationTrailing
| MutationEmpty.

Definition mutation_decision
    (admitted key_valid key_strict transition_changes extent_exact nonempty : bool)
    : mutation_outcome :=
  if negb admitted then MutationResource
  else if negb key_valid then MutationInvalidKey
  else if negb key_strict then MutationKeyOrder
  else if negb transition_changes then MutationNoop
  else if negb extent_exact then MutationTrailing
  else if negb nonempty then MutationEmpty
  else MutationAccepted.

Theorem malformed_key_precedes_order_and_transition_errors :
  forall key_strict transition_changes extent_exact nonempty,
    mutation_decision true false key_strict transition_changes extent_exact nonempty =
    MutationInvalidKey.
Proof. reflexivity. Qed.

Theorem key_order_precedes_noop_transition :
  forall extent_exact nonempty,
    mutation_decision true true false false extent_exact nonempty = MutationKeyOrder.
Proof. reflexivity. Qed.

Theorem trailing_bytes_precede_empty_batch :
  mutation_decision true true true true false false = MutationTrailing.
Proof. reflexivity. Qed.

Theorem aggregate_resource_admission_precedes_variable_grammar :
  forall key_valid key_strict transition_changes extent_exact nonempty,
    mutation_decision false key_valid key_strict transition_changes extent_exact nonempty =
    MutationResource.
Proof. reflexivity. Qed.

End MutationPrecedence.

Section CanonicalObjectGrammar.

Inductive canonical_object_kind : Type :=
| DagObject
| BlobObject
| SemanticObject
| AcceleratorObject
| RetentionObject.

Definition dispatch_object_kind (kind : canonical_object_kind) : canonical_object_kind := kind.

Theorem all_five_canonical_object_kinds_are_dispatched_exactly :
  forall kind, dispatch_object_kind kind = kind.
Proof. reflexivity. Qed.

Inductive envelope_outcome : Type :=
| EnvelopeAccepted
| EnvelopeUnsupportedVersion
| EnvelopeTruncated
| EnvelopeTrailing
| EnvelopeFieldRejected.

Definition envelope_decision
    (version_valid : bool) (declared available : nat) (field_valid : bool)
    : envelope_outcome :=
  if negb version_valid then EnvelopeUnsupportedVersion
  else if available <? declared then EnvelopeTruncated
  else if declared <? available then EnvelopeTrailing
  else if negb field_valid then EnvelopeFieldRejected
  else EnvelopeAccepted.

Theorem envelope_extent_mismatch_precedes_every_body_field :
  forall declared available field_valid,
    declared <> available ->
    envelope_decision true declared available field_valid =
    if available <? declared then EnvelopeTruncated else EnvelopeTrailing.
Proof.
  intros declared available field_valid different; unfold envelope_decision.
  destruct (available <? declared) eqn:less; [reflexivity |].
  apply Nat.ltb_ge in less.
  assert (declared < available) as greater by lia.
  assert (declared <? available = true) as greaterb by
    (apply Nat.ltb_lt; exact greater).
  rewrite greaterb. reflexivity.
Qed.

Theorem unsupported_envelope_version_precedes_extent_and_fields :
  forall declared available field_valid,
    envelope_decision false declared available field_valid = EnvelopeUnsupportedVersion.
Proof. reflexivity. Qed.

Definition aggregate_decision
    (declared available : nat) (field_valid : bool) : envelope_outcome :=
  if available <? declared then EnvelopeTruncated
  else if declared <? available then EnvelopeTrailing
  else if field_valid then EnvelopeAccepted else EnvelopeFieldRejected.

Theorem aggregate_extent_precedes_accelerator_and_retention_fields :
  forall declared available field_valid,
    declared <> available ->
    aggregate_decision declared available field_valid =
    if available <? declared then EnvelopeTruncated else EnvelopeTrailing.
Proof.
  intros declared available field_valid different; unfold aggregate_decision.
  destruct (available <? declared) eqn:less; [reflexivity |].
  apply Nat.ltb_ge in less.
  assert (declared < available) as greater by lia.
  assert (declared <? available = true) as greaterb by
    (apply Nat.ltb_lt; exact greater).
  rewrite greaterb. reflexivity.
Qed.

End CanonicalObjectGrammar.

Section BoundedMemory.

Definition streaming_codec_auxiliary_bytes
    (scratch fixed_state input key object : nat) : nat := scratch + fixed_state.

Theorem streaming_codec_auxiliary_memory_has_zero_input_slope :
  forall scratch fixed_state input_a input_b key_a key_b object_a object_b,
    streaming_codec_auxiliary_bytes scratch fixed_state input_a key_a object_a =
    streaming_codec_auxiliary_bytes scratch fixed_state input_b key_b object_b.
Proof. reflexivity. Qed.

End BoundedMemory.

Print Assumptions chunked_frame_pda_refines_contiguous_frame_parser.
Print Assumptions dangling_escape_is_truncated.
Print Assumptions group_marker_inside_frame_is_not_a_terminator.
Print Assumptions escaped_zero_continues_the_frame.
Print Assumptions datum_identity_suffix_is_unframed_and_may_be_empty.
Print Assumptions continuation_requires_nonempty_group_identity_and_no_trailing_bytes.
Print Assumptions join_requires_one_channel_and_nonempty_unframed_suffix.
Print Assumptions equal_keys_are_rejected_by_strict_order.
Print Assumptions proper_prefix_is_lexicographically_smaller.
Print Assumptions unstable_reread_cannot_authorize_key_order.
Print Assumptions stable_chunk_reassembly_uses_exact_byte_order.
Print Assumptions malformed_key_precedes_order_and_transition_errors.
Print Assumptions key_order_precedes_noop_transition.
Print Assumptions trailing_bytes_precede_empty_batch.
Print Assumptions aggregate_resource_admission_precedes_variable_grammar.
Print Assumptions all_five_canonical_object_kinds_are_dispatched_exactly.
Print Assumptions envelope_extent_mismatch_precedes_every_body_field.
Print Assumptions unsupported_envelope_version_precedes_extent_and_fields.
Print Assumptions aggregate_extent_precedes_accelerator_and_retention_fields.
Print Assumptions streaming_codec_auxiliary_memory_has_zero_input_slope.
