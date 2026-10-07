From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import SignedPhloControls.
Import ListNotations.

Record economic_failure_summary := {
  failure_user : bool;
  failure_platform : bool;
  failure_certificate : bool;
  failure_unclassified : bool
}.

Definition no_economic_failures :=
  {| failure_user := false; failure_platform := false;
     failure_certificate := false; failure_unclassified := false |}.

Definition join_economic_failures left right :=
  {| failure_user := failure_user left || failure_user right;
     failure_platform := failure_platform left || failure_platform right;
     failure_certificate := failure_certificate left || failure_certificate right;
     failure_unclassified := failure_unclassified left || failure_unclassified right |}.

Definition economic_failure_bit failure :=
  match failure with
  | PhloUserFailure => {| failure_user := true; failure_platform := false;
      failure_certificate := false; failure_unclassified := false |}
  | PhloPlatformFailure => {| failure_user := false; failure_platform := true;
      failure_certificate := false; failure_unclassified := false |}
  | PhloCertificateFailure => {| failure_user := false; failure_platform := false;
      failure_certificate := true; failure_unclassified := false |}
  | PhloUnclassifiedFailure => {| failure_user := false; failure_platform := false;
      failure_certificate := false; failure_unclassified := true |}
  end.

Definition economic_summary failures :=
  fold_right (fun failure summary => join_economic_failures (economic_failure_bit failure) summary)
    no_economic_failures failures.

Definition economic_veto summary :=
  failure_platform summary || failure_certificate summary || failure_unclassified summary.

Definition economic_bits summary :=
  (if failure_user summary then 1 else 0) +
  (if failure_platform summary then 2 else 0) +
  (if failure_certificate summary then 4 else 0) +
  (if failure_unclassified summary then 8 else 0).

Theorem economic_join_associative : forall a b c,
  join_economic_failures a (join_economic_failures b c) =
  join_economic_failures (join_economic_failures a b) c.
Proof. intros [a b c d] [e f g h] [i j k l]; unfold join_economic_failures; simpl.
  now rewrite !orb_assoc. Qed.

Theorem economic_join_commutative : forall a b,
  join_economic_failures a b = join_economic_failures b a.
Proof. intros [a b c d] [e f g h]; unfold join_economic_failures; simpl.
  now rewrite (orb_comm a e), (orb_comm b f), (orb_comm c g), (orb_comm d h). Qed.

Theorem economic_join_idempotent : forall a, join_economic_failures a a = a.
Proof. intros [a b c d]; unfold join_economic_failures; simpl. now rewrite !orb_diag. Qed.

Theorem economic_join_zero : forall a, join_economic_failures no_economic_failures a = a.
Proof. intros [a b c d]; reflexivity. Qed.

Theorem economic_join_refines_bitwise_or : forall a b,
  economic_bits (join_economic_failures a b) = Nat.lor (economic_bits a) (economic_bits b).
Proof.
  intros [a b c d] [e f g h].
  destruct a, b, c, d, e, f, g, h; reflexivity.
Qed.

Theorem economic_summary_fits_atomic_u8 : forall summary, economic_bits summary < 256.
Proof. intros [a b c d]; destruct a, b, c, d; unfold economic_bits; simpl; lia. Qed.

Theorem economic_summary_append : forall left right,
  economic_summary (left ++ right) =
    join_economic_failures (economic_summary left) (economic_summary right).
Proof.
  induction left as [|failure rest IH]; intros right; simpl.
  - symmetry. apply economic_join_zero.
  - change (join_economic_failures (economic_failure_bit failure) (economic_summary (rest ++ right)) =
      join_economic_failures (join_economic_failures (economic_failure_bit failure)
        (economic_summary rest)) (economic_summary right)).
    rewrite IH, economic_join_associative. reflexivity.
Qed.

Theorem economic_summary_permutation_invariant : forall left right,
  Permutation left right -> economic_summary left = economic_summary right.
Proof.
  intros left right order. induction order; simpl; try congruence.
  rewrite !economic_join_associative, (economic_join_commutative (economic_failure_bit x)). reflexivity.
Qed.

Theorem economic_summary_nested_join_invariant : forall groups,
  economic_summary (concat groups) =
  fold_right join_economic_failures no_economic_failures (map economic_summary groups).
Proof. induction groups as [|group rest IH]; simpl; [reflexivity|].
  rewrite economic_summary_append, IH. reflexivity. Qed.

Theorem economic_completed_atomic_update_history_exact : forall failures initial,
  fold_left (fun summary failure => join_economic_failures summary (economic_failure_bit failure))
    failures initial = join_economic_failures initial (economic_summary failures).
Proof.
  induction failures as [|failure rest IH]; intros initial; simpl.
  - rewrite economic_join_commutative. symmetry. apply economic_join_zero.
  - rewrite IH. unfold economic_summary at 2. simpl fold_right.
    fold (economic_summary rest). symmetry. apply economic_join_associative.
Qed.

Theorem economic_summary_billability_exact : forall failures,
  negb (economic_veto (economic_summary failures)) = forallb billable_failure failures.
Proof.
  induction failures as [|failure rest IH]; [reflexivity|].
  unfold economic_summary at 1. simpl fold_right. fold (economic_summary rest).
  destruct failure; simpl in *.
  - exact IH.
  - reflexivity.
  - unfold economic_veto, join_economic_failures. simpl. now rewrite orb_true_r.
  - unfold economic_veto, join_economic_failures. simpl. now rewrite orb_true_r.
Qed.

Definition summary_retained_phlo_charge schedule fresh summary :=
  if economic_veto summary then 0 else newly_required_charge schedule fresh.

Theorem economic_summary_refines_retained_charge : forall schedule fresh failures,
  summary_retained_phlo_charge schedule fresh (economic_summary failures) =
  retained_phlo_charge schedule fresh (PhloAccepted failures).
Proof.
  intros. unfold summary_retained_phlo_charge, retained_phlo_charge.
  rewrite <- economic_summary_billability_exact.
  destruct (economic_veto (economic_summary failures)); reflexivity.
Qed.

Theorem economic_nonuser_veto_survives_all_siblings : forall schedule fresh failures bad,
  In bad failures -> billable_failure bad = false ->
  summary_retained_phlo_charge schedule fresh (economic_summary failures) = 0.
Proof.
  intros. rewrite economic_summary_refines_retained_charge.
  eapply unsafe_failure_prevents_all_candidate_charge; eauto.
Qed.

Definition normalized_error_aggregate failures :=
  match failures with [] => [PhloUnclassifiedFailure] | _ => failures end.

Theorem empty_error_aggregate_is_nonbillable : forall schedule fresh,
  summary_retained_phlo_charge schedule fresh
    (economic_summary (normalized_error_aggregate [])) = 0.
Proof. reflexivity. Qed.

Definition legacy_user_priority failures :=
  if existsb billable_failure failures then [PhloUserFailure] else failures.

Theorem legacy_user_priority_loses_nonbillable_sibling : forall schedule fresh,
  retained_phlo_charge schedule fresh
    (PhloAccepted (legacy_user_priority [PhloUserFailure; PhloPlatformFailure])) =
    newly_required_charge schedule fresh /\
  summary_retained_phlo_charge schedule fresh
    (economic_summary [PhloUserFailure; PhloPlatformFailure]) = 0 /\
  0 < newly_required_charge schedule fresh.
Proof.
  intros. repeat split; try reflexivity.
  unfold newly_required_charge. lia.
Qed.

Definition economic_sessions := nat -> option economic_failure_summary.

Definition start_economic_session (sessions : economic_sessions) identity :=
  match sessions identity with
  | Some _ => None
  | None => Some (fun key => if Nat.eqb key identity then Some no_economic_failures else sessions key)
  end.

Definition record_session_failure (sessions : economic_sessions) identity failure :=
  fun key => if Nat.eqb key identity then
    match sessions identity with
    | None => None
    | Some observed => Some (join_economic_failures observed (economic_failure_bit failure))
    end
  else sessions key.

Theorem fresh_session_starts_empty_without_resetting_others : forall sessions identity next,
  start_economic_session sessions identity = Some next ->
  sessions identity = None /\ next identity = Some no_economic_failures /\
  forall other, other <> identity -> next other = sessions other.
Proof.
  intros sessions identity next started. unfold start_economic_session in started.
  destruct (sessions identity) eqn:absent; [discriminate|]. inversion started; subst next.
  split; [reflexivity|]. split; [now rewrite Nat.eqb_refl|].
  intros other distinct. apply Nat.eqb_neq in distinct. now rewrite distinct.
Qed.

Theorem late_old_session_report_cannot_change_new_session : forall sessions old fresh failure,
  old <> fresh -> record_session_failure sessions old failure fresh = sessions fresh.
Proof.
  intros sessions old fresh failure distinct. unfold record_session_failure.
  assert (fresh =? old = false) by (apply Nat.eqb_neq; congruence). now rewrite H.
Qed.

Theorem session_report_keeps_own_prior_failures : forall sessions identity observed failure,
  sessions identity = Some observed ->
  record_session_failure sessions identity failure identity =
    Some (join_economic_failures observed (economic_failure_bit failure)).
Proof. intros. unfold record_session_failure. now rewrite Nat.eqb_refl, H. Qed.

Theorem live_session_cannot_be_reset_by_creation : forall sessions identity observed,
  sessions identity = Some observed -> start_economic_session sessions identity = None.
Proof. intros. unfold start_economic_session. now rewrite H. Qed.

Theorem classifier_incomplete_summary_is_nonbillable : forall summary schedule fresh,
  summary_retained_phlo_charge schedule fresh
    (join_economic_failures summary (economic_failure_bit PhloPlatformFailure)) = 0.
Proof.
  intros [a b c d] schedule fresh. unfold summary_retained_phlo_charge,
    economic_veto, join_economic_failures, economic_failure_bit. simpl.
  now rewrite orb_true_r.
Qed.

Print Assumptions economic_join_associative.
Print Assumptions economic_join_commutative.
Print Assumptions economic_join_idempotent.
Print Assumptions economic_join_refines_bitwise_or.
Print Assumptions economic_summary_fits_atomic_u8.
Print Assumptions economic_summary_append.
Print Assumptions economic_summary_permutation_invariant.
Print Assumptions economic_summary_nested_join_invariant.
Print Assumptions economic_completed_atomic_update_history_exact.
Print Assumptions economic_summary_billability_exact.
Print Assumptions economic_summary_refines_retained_charge.
Print Assumptions economic_nonuser_veto_survives_all_siblings.
Print Assumptions empty_error_aggregate_is_nonbillable.
Print Assumptions legacy_user_priority_loses_nonbillable_sibling.
Print Assumptions fresh_session_starts_empty_without_resetting_others.
Print Assumptions late_old_session_report_cannot_change_new_session.
Print Assumptions session_report_keeps_own_prior_failures.
Print Assumptions live_session_cannot_be_reset_by_creation.
Print Assumptions classifier_incomplete_summary_is_nonbillable.
