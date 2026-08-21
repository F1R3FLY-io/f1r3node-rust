From Stdlib Require Import Arith.Arith.

From FinalizedFloor Require Import MergeRecoveryCoherence.

Inductive disposition_reason :=
| ReasonUnspecified
| ReasonWon
| ReasonRejected.

Record disposition_record := {
  record_provenance : option nat;
  record_reason : disposition_reason
}.

Definition exact_protocol (version : nat) : Prop := 2 <= version.

Definition encoding_matches
  (version : nat)
  (record : disposition_record) : Prop :=
  if Nat.leb 2 version then
    (exists provenance, record_provenance record = Some provenance) /\
    record_reason record <> ReasonUnspecified
  else
    record_provenance record = None /\
    record_reason record = ReasonUnspecified.

Definition scope_admissible
  (active_version block_version : nat)
  (record : disposition_record) : Prop :=
  block_version = active_version /\ encoding_matches block_version record.

Definition protocol_selected
  (active_version block_version : nat)
  (record : disposition_record)
  (base scope : receipt_set)
  (tombstones : tombstone_set)
  (candidate : effect_receipt) : Prop :=
  scope_admissible active_version block_version record /\
  selected base scope tombstones candidate.

Theorem admissible_scope_uses_active_version :
  forall active_version block_version record,
    scope_admissible active_version block_version record ->
    block_version = active_version.
Proof.
  intros active_version block_version record Hadmissible.
  exact (proj1 Hadmissible).
Qed.

Theorem exact_encoding_requires_provenance :
  forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    exists provenance, record_provenance record = Some provenance.
Proof.
  intros version record Hexact Hencoding.
  unfold exact_protocol in Hexact.
  unfold encoding_matches in Hencoding.
  assert (Nat.leb 2 version = true) as Htrue.
  { apply Nat.leb_le. exact Hexact. }
  rewrite Htrue in Hencoding.
  exact (proj1 Hencoding).
Qed.

Theorem exact_encoding_requires_reason :
  forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    record_reason record <> ReasonUnspecified.
Proof.
  intros version record Hexact Hencoding.
  unfold exact_protocol in Hexact.
  unfold encoding_matches in Hencoding.
  assert (Nat.leb 2 version = true) as Htrue.
  { apply Nat.leb_le. exact Hexact. }
  rewrite Htrue in Hencoding.
  exact (proj2 Hencoding).
Qed.

Theorem legacy_encoding_forbids_provenance :
  forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_provenance record = None.
Proof.
  intros version record Hlegacy Hencoding.
  unfold encoding_matches in Hencoding.
  assert (Nat.leb 2 version = false) as Hfalse.
  { apply Nat.leb_gt. exact Hlegacy. }
  rewrite Hfalse in Hencoding.
  exact (proj1 Hencoding).
Qed.

Theorem legacy_encoding_requires_unspecified_reason :
  forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_reason record = ReasonUnspecified.
Proof.
  intros version record Hlegacy Hencoding.
  unfold encoding_matches in Hencoding.
  assert (Nat.leb 2 version = false) as Hfalse.
  { apply Nat.leb_gt. exact Hlegacy. }
  rewrite Hfalse in Hencoding.
  exact (proj2 Hencoding).
Qed.

Theorem legacy_floor_exact_activation_preserves_base_dominance :
  forall active_version floor_version block_version record,
  forall base scope tombstones committed_receipt candidate,
    exact_protocol active_version ->
    floor_version < 2 ->
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ protocol_selected active_version block_version record
        base scope tombstones candidate.
Proof.
  intros active_version floor_version block_version record.
  intros base scope tombstones committed_receipt candidate.
  intros _ _ Hbase Hdeploy Hselected.
  destruct Hselected as [_ Hselected].
  eapply (base_committed_dominates_scope
    base scope tombstones committed_receipt candidate Hbase Hdeploy).
  exact Hselected.
Qed.

Print Assumptions admissible_scope_uses_active_version.
Print Assumptions exact_encoding_requires_provenance.
Print Assumptions exact_encoding_requires_reason.
Print Assumptions legacy_encoding_forbids_provenance.
Print Assumptions legacy_encoding_requires_unspecified_reason.
Print Assumptions legacy_floor_exact_activation_preserves_base_dominance.
