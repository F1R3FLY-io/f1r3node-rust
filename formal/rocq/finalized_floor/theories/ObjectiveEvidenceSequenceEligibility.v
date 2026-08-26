From Stdlib Require Import Bool.Bool.
From Stdlib Require Import ZArith.

Inductive certified_admission : Type :=
| EvidenceAccepted
| EvidenceObjectiveRejected
| EvidenceUnattributable.

Definition persists_metadata (admission : certified_admission) : bool :=
  match admission with
  | EvidenceAccepted | EvidenceObjectiveRejected => true
  | EvidenceUnattributable => false
  end.

Definition evidence_sequence_eligible (sequence : Z) : bool :=
  Z.leb 0 sequence.

Definition indexes_objective_evidence
  (admission : certified_admission)
  (sequence : Z) : bool :=
  andb (persists_metadata admission) (evidence_sequence_eligible sequence).

Theorem attributable_negative_sequence_persists_without_evidence :
  forall sequence : Z,
    (sequence < 0)%Z ->
    persists_metadata EvidenceObjectiveRejected = true /\
    indexes_objective_evidence EvidenceObjectiveRejected sequence = false.
Proof.
  intros sequence Hnegative.
  split; simpl.
  - reflexivity.
  - apply Z.leb_gt.
    exact Hnegative.
Qed.

Theorem indexed_evidence_has_attributable_nonnegative_sequence :
  forall admission (sequence : Z),
    indexes_objective_evidence admission sequence = true ->
    persists_metadata admission = true /\ (0 <= sequence)%Z.
Proof.
  intros admission sequence Hindexed.
  unfold indexes_objective_evidence in Hindexed.
  apply andb_true_iff in Hindexed.
  destruct Hindexed as [Hpersist Heligible].
  split.
  - exact Hpersist.
  - unfold evidence_sequence_eligible in Heligible.
    apply Z.leb_le.
    exact Heligible.
Qed.

Theorem nonnegative_certified_admission_is_indexable :
  forall admission (sequence : Z),
    persists_metadata admission = true ->
    (0 <= sequence)%Z ->
    indexes_objective_evidence admission sequence = true.
Proof.
  intros admission sequence Hpersist Hsequence.
  unfold indexes_objective_evidence, evidence_sequence_eligible.
  rewrite Hpersist.
  apply Z.leb_le.
  exact Hsequence.
Qed.
