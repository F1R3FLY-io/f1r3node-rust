From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Inductive user_disposition :=
| Executed
| ExecutionFailed
| AdmissionRejected.

Record user_record := {
  user_record_id : nat;
  user_record_disposition : user_disposition
}.

Definition effect_bearing (record : user_record) : bool :=
  match user_record_disposition record with
  | Executed => true
  | ExecutionFailed => true
  | AdmissionRejected => false
  end.

Definition user_effect_records (records : list user_record) : list user_record :=
  filter effect_bearing records.

Definition required_merge_metadata
  (records : list user_record)
  (system_effect_count : nat) : nat :=
  length (user_effect_records records) + system_effect_count.

Definition metadata_aligned
  (records : list user_record)
  (system_effect_count : nat)
  (metadata : list nat) : Prop :=
  length metadata = required_merge_metadata records system_effect_count.

Lemma user_effect_records_app :
  forall left right,
    user_effect_records (left ++ right) =
    user_effect_records left ++ user_effect_records right.
Proof.
  intros left right.
  unfold user_effect_records.
  apply filter_app.
Qed.

Theorem admission_rejected_has_no_effect_slot :
  forall left right record_id,
    user_effect_records
      (left ++ {| user_record_id := record_id;
                  user_record_disposition := AdmissionRejected |} :: right) =
    user_effect_records (left ++ right).
Proof.
  intros left right record_id.
  repeat rewrite user_effect_records_app.
  reflexivity.
Qed.

Theorem executed_failure_retains_effect_slot :
  forall record_id,
    length
      (user_effect_records
        [{| user_record_id := record_id;
            user_record_disposition := ExecutionFailed |}]) = 1.
Proof.
  intros record_id.
  reflexivity.
Qed.

Theorem effect_projection_permutation_length :
  forall left right,
    Permutation left right ->
    length (user_effect_records left) = length (user_effect_records right).
Proof.
  intros left right Hpermutation.
  induction Hpermutation.
  - reflexivity.
  - unfold user_effect_records in *.
    simpl.
    destruct (effect_bearing x); simpl; now rewrite IHHpermutation.
  - unfold user_effect_records.
    simpl.
    destruct (effect_bearing x), (effect_bearing y); reflexivity.
  - now rewrite IHHpermutation1, IHHpermutation2.
Qed.

Theorem aligned_metadata_splits_exactly :
  forall records system_effect_count metadata,
    metadata_aligned records system_effect_count metadata ->
    exists user_metadata system_metadata,
      metadata = user_metadata ++ system_metadata /\
      length user_metadata = length (user_effect_records records) /\
      length system_metadata = system_effect_count.
Proof.
  intros records system_effect_count metadata Haligned.
  exists (firstn (length (user_effect_records records)) metadata).
  exists (skipn (length (user_effect_records records)) metadata).
  repeat split.
  - symmetry. apply firstn_skipn.
  - rewrite length_firstn.
    unfold metadata_aligned, required_merge_metadata in Haligned.
    rewrite Haligned.
    rewrite Nat.min_l; lia.
  - rewrite length_skipn.
    unfold metadata_aligned, required_merge_metadata in Haligned.
    rewrite Haligned.
    lia.
Qed.

Theorem admission_rejection_close_block_regression :
  forall record_id,
    required_merge_metadata
      [{| user_record_id := record_id;
          user_record_disposition := AdmissionRejected |}]
      1 = 1 /\
    length
      [{| user_record_id := record_id;
          user_record_disposition := AdmissionRejected |}] + 1 = 2.
Proof.
  intros record_id.
  split; reflexivity.
Qed.

Print Assumptions admission_rejected_has_no_effect_slot.
Print Assumptions executed_failure_retains_effect_slot.
Print Assumptions effect_projection_permutation_length.
Print Assumptions aligned_metadata_splits_exactly.
Print Assumptions admission_rejection_close_block_regression.
