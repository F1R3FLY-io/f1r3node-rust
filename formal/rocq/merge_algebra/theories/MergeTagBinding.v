From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Inductive MergeKind : Type :=
| Ordinary
| IntegerAdd
| BitmaskOr.

Inductive SystemMergeUri : Type :=
| IntegerAddUri
| BitmaskOrUri.

Definition Tag := nat.
Definition EnvelopeIdentity := nat.

Definition integer_add_tag : Tag := 0.
Definition bitmask_or_tag : Tag := 1.

Definition resolve_system_uri (uri : SystemMergeUri) : Tag :=
  match uri with
  | IntegerAddUri => integer_add_tag
  | BitmaskOrUri => bitmask_or_tag
  end.

Definition classify_tag (tag : Tag) : MergeKind :=
  if Nat.eqb tag integer_add_tag then IntegerAdd
  else if Nat.eqb tag bitmask_or_tag then BitmaskOr
  else Ordinary.

Definition safe_numeric_contract_tag (_ : EnvelopeIdentity) : Tag :=
  resolve_system_uri IntegerAddUri.

Definition legacy_fresh_contract_tag (envelope : EnvelopeIdentity) : Tag :=
  envelope.

Definition safe_numeric_tags (envelopes : list EnvelopeIdentity) : list Tag :=
  map safe_numeric_contract_tag envelopes.

Definition safe_numeric_kinds (envelopes : list EnvelopeIdentity) : list MergeKind :=
  map (fun envelope => classify_tag (safe_numeric_contract_tag envelope)) envelopes.

Theorem system_merge_uris_are_separated :
  resolve_system_uri IntegerAddUri <> resolve_system_uri BitmaskOrUri.
Proof. discriminate. Qed.

Theorem integer_add_classification_is_authenticated :
  forall tag,
    classify_tag tag = IntegerAdd <-> tag = integer_add_tag.
Proof.
  intros tag. unfold classify_tag.
  destruct (Nat.eqb tag integer_add_tag) eqn:E.
  - split; [intro; apply Nat.eqb_eq; exact E | reflexivity].
  - split.
    + destruct (Nat.eqb tag bitmask_or_tag); discriminate.
    + intro H. subst tag. rewrite Nat.eqb_refl in E. discriminate.
Qed.

Theorem safe_numeric_contract_uses_configured_tag :
  forall envelope,
    safe_numeric_contract_tag envelope = integer_add_tag.
Proof. reflexivity. Qed.

Theorem safe_numeric_tag_is_envelope_independent :
  forall left right,
    safe_numeric_contract_tag left = safe_numeric_contract_tag right.
Proof. reflexivity. Qed.

Theorem safe_numeric_event_is_integer_add :
  forall envelope,
    classify_tag (safe_numeric_contract_tag envelope) = IntegerAdd.
Proof. reflexivity. Qed.

Theorem parallel_numeric_tags_are_identical :
  forall envelopes,
    safe_numeric_tags envelopes = repeat integer_add_tag (length envelopes).
Proof.
  induction envelopes as [| envelope rest IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Theorem parallel_numeric_classification_is_identical :
  forall envelopes,
    safe_numeric_kinds envelopes = repeat IntegerAdd (length envelopes).
Proof.
  induction envelopes as [| envelope rest IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Theorem parallel_numeric_classification_is_permutation_invariant :
  forall left right,
    Permutation left right ->
    safe_numeric_kinds left = safe_numeric_kinds right.
Proof.
  intros left right Hperm.
  rewrite !parallel_numeric_classification_is_identical.
  now rewrite (Permutation_length Hperm).
Qed.

Theorem legacy_protocol_identity_change_breaks_binding :
  legacy_fresh_contract_tag 0 = integer_add_tag
  /\ legacy_fresh_contract_tag 1 <> integer_add_tag
  /\ classify_tag (legacy_fresh_contract_tag 1) <> IntegerAdd.
Proof.
  repeat split; discriminate.
Qed.

Theorem merge_tag_binding_correct :
  (forall tag, classify_tag tag = IntegerAdd <-> tag = integer_add_tag)
  /\ (forall envelope,
        safe_numeric_contract_tag envelope = integer_add_tag
        /\ classify_tag (safe_numeric_contract_tag envelope) = IntegerAdd)
  /\ (forall left right,
        safe_numeric_contract_tag left = safe_numeric_contract_tag right)
  /\ (forall envelopes,
        safe_numeric_kinds envelopes = repeat IntegerAdd (length envelopes))
  /\ (forall left right,
        Permutation left right ->
        safe_numeric_kinds left = safe_numeric_kinds right).
Proof.
  split.
  - exact integer_add_classification_is_authenticated.
  - split.
    + intro envelope. split.
      * apply safe_numeric_contract_uses_configured_tag.
      * apply safe_numeric_event_is_integer_add.
    + split.
      * exact safe_numeric_tag_is_envelope_independent.
      * split.
        -- exact parallel_numeric_classification_is_identical.
        -- exact parallel_numeric_classification_is_permutation_invariant.
Qed.

Print Assumptions merge_tag_binding_correct.
Print Assumptions legacy_protocol_identity_change_breaks_binding.
