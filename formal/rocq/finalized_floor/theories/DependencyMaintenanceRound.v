From Stdlib Require Import Lists.List.
Import ListNotations.

Section DependencyMaintenanceRound.

Context {Block Digest : Type}.

Inductive maintenance_obligation : Type :=
| BlockRequest : Block -> maintenance_obligation
| CertificateRequest : Digest -> maintenance_obligation.

Variable attempt : maintenance_obligation -> bool.

Fixpoint dispatch_round
  (work : list maintenance_obligation)
  : list maintenance_obligation * option maintenance_obligation :=
  match work with
  | [] => ([], None)
  | obligation :: tail =>
      let '(attempted, first_error) := dispatch_round tail in
      (obligation :: attempted,
       if attempt obligation then first_error else Some obligation)
  end.

Theorem dispatch_round_attempts_exact_snapshot :
  forall work, fst (dispatch_round work) = work.
Proof.
  induction work as [| obligation tail induction].
  - reflexivity.
  - simpl.
    destruct (dispatch_round tail) as [attempted first_error] eqn:round.
    simpl.
    simpl in induction.
    now rewrite induction.
Qed.

Theorem every_snapshot_obligation_is_attempted :
  forall work obligation,
    In obligation work -> In obligation (fst (dispatch_round work)).
Proof.
  intros work obligation present.
  rewrite dispatch_round_attempts_exact_snapshot.
  exact present.
Qed.

Theorem first_error_names_a_failed_attempt :
  forall work obligation,
    snd (dispatch_round work) = Some obligation ->
    In obligation work /\ attempt obligation = false.
Proof.
  induction work as [| head tail induction].
  - simpl.
    discriminate.
  - simpl.
    destruct (dispatch_round tail) as [attempted first_error] eqn:round.
    destruct (attempt head) eqn:head_result.
    + simpl.
      intros obligation result.
      specialize (induction obligation).
      simpl in induction.
      destruct (induction result) as [present failed].
      split.
      * right.
        exact present.
      * exact failed.
    + simpl.
      intros obligation result.
      inversion result.
      subst obligation.
      split.
      * left.
        reflexivity.
      * exact head_result.
Qed.

Theorem failed_block_attempt_cannot_suppress_certificate_attempt :
  forall work block digest,
    In (BlockRequest block) work ->
    attempt (BlockRequest block) = false ->
    In (CertificateRequest digest) work ->
    In (CertificateRequest digest) (fst (dispatch_round work)).
Proof.
  intros work block digest block_present block_failed certificate_present.
  apply every_snapshot_obligation_is_attempted.
  exact certificate_present.
Qed.

Theorem dependency_maintenance_round_contract :
  (forall work, fst (dispatch_round work) = work)
  /\
  (forall work obligation,
    In obligation work -> In obligation (fst (dispatch_round work)))
  /\
  (forall work obligation,
    snd (dispatch_round work) = Some obligation ->
    In obligation work /\ attempt obligation = false)
  /\
  (forall work block digest,
    In (BlockRequest block) work ->
    attempt (BlockRequest block) = false ->
    In (CertificateRequest digest) work ->
    In (CertificateRequest digest) (fst (dispatch_round work))).
Proof.
  split.
  - exact dispatch_round_attempts_exact_snapshot.
  - split.
    + exact every_snapshot_obligation_is_attempted.
    + split.
      * exact first_error_names_a_failed_attempt.
      * exact failed_block_attempt_cannot_suppress_certificate_attempt.
Qed.

End DependencyMaintenanceRound.

Print Assumptions dependency_maintenance_round_contract.
