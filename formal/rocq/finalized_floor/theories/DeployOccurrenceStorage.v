From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import ZArith.ZArith.
Import ListNotations.

Definition ingress_window_open
  (valid_after next_block lifespan : Z) : Prop :=
  (valid_after > next_block - lifespan)%Z.

Theorem ingress_window_matches_lifespan_upper_bound :
  forall valid_after next_block lifespan,
    (0 <= lifespan)%Z ->
    ingress_window_open valid_after next_block lifespan <->
    (next_block < valid_after + lifespan)%Z.
Proof.
  intros valid_after next_block lifespan Hlifespan.
  unfold ingress_window_open.
  lia.
Qed.

Theorem ingress_window_boundary_is_closed :
  forall valid_after lifespan,
    (0 <= lifespan)%Z ->
    ~ ingress_window_open valid_after (valid_after + lifespan) lifespan.
Proof.
  intros valid_after lifespan Hlifespan.
  unfold ingress_window_open.
  lia.
Qed.

Theorem closed_ingress_window_stays_closed_after_tip_advance :
  forall valid_after observed_tip later_tip lifespan,
    (observed_tip <= later_tip)%Z ->
    ~ ingress_window_open valid_after observed_tip lifespan ->
    ~ ingress_window_open valid_after later_tip lifespan.
Proof.
  intros valid_after observed_tip later_tip lifespan Htip Hclosed.
  unfold ingress_window_open in *.
  lia.
Qed.

Theorem deploy_ingress_window_contract :
  (forall valid_after next_block lifespan,
    (0 <= lifespan)%Z ->
    ingress_window_open valid_after next_block lifespan <->
    (next_block < valid_after + lifespan)%Z) /\
  (forall valid_after lifespan,
    (0 <= lifespan)%Z ->
    ~ ingress_window_open valid_after (valid_after + lifespan) lifespan) /\
  (forall valid_after observed_tip later_tip lifespan,
    (observed_tip <= later_tip)%Z ->
    ~ ingress_window_open valid_after observed_tip lifespan ->
    ~ ingress_window_open valid_after later_tip lifespan).
Proof.
  split.
  - apply ingress_window_matches_lifespan_upper_bound.
  - split.
    + apply ingress_window_boundary_is_closed.
    + apply closed_ingress_window_stays_closed_after_tip_advance.
Qed.

Inductive deploy_lookup_id :=
| LegacyDeployId : nat -> deploy_lookup_id
| DeployIdV6 : nat -> deploy_lookup_id.

Theorem legacy_and_v6_identities_are_disjoint :
  forall legacy commitment,
    LegacyDeployId legacy <> DeployIdV6 commitment.
Proof.
  discriminate.
Qed.

Definition deploy_lookup_id_eq_dec :
  forall left right : deploy_lookup_id, {left = right} + {left <> right}.
Proof.
  decide equality; apply Nat.eq_dec.
Defined.

Record stored_occurrence := {
  occurrence_id : deploy_lookup_id;
  occurrence_source : nat;
  occurrence_rank : nat
}.

Definition stored_occurrence_eq_dec :
  forall left right : stored_occurrence, {left = right} + {left <> right}.
Proof.
  decide equality; try apply Nat.eq_dec; apply deploy_lookup_id_eq_dec.
Defined.

Definition insert_archive
  (row : stored_occurrence)
  (archive : list stored_occurrence) : list stored_occurrence :=
  if in_dec stored_occurrence_eq_dec row archive
  then archive
  else row :: archive.

Theorem archive_insert_is_idempotent :
  forall row archive,
    insert_archive row (insert_archive row archive) = insert_archive row archive.
Proof.
  intros row archive.
  unfold insert_archive at 1.
  destruct (in_dec stored_occurrence_eq_dec row (insert_archive row archive)).
  - reflexivity.
  - exfalso.
    apply n.
    unfold insert_archive.
    destruct (in_dec stored_occurrence_eq_dec row archive).
    + exact i.
    + simpl. left. reflexivity.
Qed.

Theorem archive_insert_preserves_existing_rows :
  forall row existing archive,
    In existing archive -> In existing (insert_archive row archive).
Proof.
  intros row existing archive Hin.
  unfold insert_archive.
  destruct (in_dec stored_occurrence_eq_dec row archive).
  - exact Hin.
  - simpl. right. exact Hin.
Qed.

Theorem archive_insert_contains_inserted_row :
  forall row archive,
    In row (insert_archive row archive).
Proof.
  intros row archive.
  unfold insert_archive.
  destruct (in_dec stored_occurrence_eq_dec row archive).
  - exact i.
  - simpl. left. reflexivity.
Qed.

Fixpoint canonical_rank (archive : list stored_occurrence) : nat :=
  match archive with
  | [] => 0
  | row :: rest => Nat.max (occurrence_rank row) (canonical_rank rest)
  end.

Theorem rank_reducer_is_commutative :
  forall left right,
    Nat.max left right = Nat.max right left.
Proof.
  exact Nat.max_comm.
Qed.

Theorem rank_reducer_is_associative :
  forall first second third,
    Nat.max first (Nat.max second third) = Nat.max (Nat.max first second) third.
Proof.
  intros first second third.
  apply Nat.max_assoc.
Qed.

Theorem rank_reducer_is_idempotent :
  forall rank,
    Nat.max rank rank = rank.
Proof.
  intro rank.
  apply Nat.max_id.
Qed.

Theorem canonical_rank_is_permutation_invariant :
  forall left right,
    Permutation left right -> canonical_rank left = canonical_rank right.
Proof.
  intros left right Hpermutation.
  induction Hpermutation.
  - reflexivity.
  - simpl. now rewrite IHHpermutation.
  - simpl. repeat rewrite Nat.max_assoc.
    rewrite (Nat.max_comm (occurrence_rank y) (occurrence_rank x)).
    reflexivity.
  - now rewrite IHHpermutation1, IHHpermutation2.
Qed.

Inductive terminal_state := Finalized | Expired | Failed.

Record occurrence_storage := {
  archived_rows : list stored_occurrence;
  active_row : option stored_occurrence;
  open_summary : option stored_occurrence;
  terminal_summary : option terminal_state;
  lifecycle_open : bool;
  lifecycle_terminal : option terminal_state
}.

Definition terminalize
  (state : terminal_state)
  (store : occurrence_storage) : occurrence_storage :=
  {| archived_rows := archived_rows store;
     active_row := None;
     open_summary := None;
     terminal_summary := Some state;
     lifecycle_open := false;
     lifecycle_terminal := Some state |}.

Theorem terminalization_preserves_exact_archive :
  forall state store,
    archived_rows (terminalize state store) = archived_rows store.
Proof.
  reflexivity.
Qed.

Theorem terminalization_is_atomic_across_occurrence_and_lifecycle_state :
  forall state store,
    active_row (terminalize state store) = None /\
    open_summary (terminalize state store) = None /\
    terminal_summary (terminalize state store) = Some state /\
    lifecycle_open (terminalize state store) = false /\
    lifecycle_terminal (terminalize state store) = Some state.
Proof.
  repeat split; reflexivity.
Qed.

Definition fresh_activation_allowed
  (legacy_rows partial_rows : nat) : bool :=
  Nat.eqb legacy_rows 0 && Nat.eqb partial_rows 0.

Theorem successful_activation_has_no_legacy_or_partial_rows :
  forall legacy_rows partial_rows,
    fresh_activation_allowed legacy_rows partial_rows = true ->
    legacy_rows = 0 /\ partial_rows = 0.
Proof.
  intros legacy_rows partial_rows Hallowed.
  unfold fresh_activation_allowed in Hallowed.
  apply andb_true_iff in Hallowed.
  destruct Hallowed as [Hlegacy Hpartial].
  apply Nat.eqb_eq in Hlegacy.
  apply Nat.eqb_eq in Hpartial.
  now split.
Qed.

Theorem deploy_occurrence_storage_contract :
  (forall legacy commitment,
    LegacyDeployId legacy <> DeployIdV6 commitment) /\
  (forall row archive,
    insert_archive row (insert_archive row archive) = insert_archive row archive) /\
  (forall left right,
    Permutation left right -> canonical_rank left = canonical_rank right) /\
  (forall state store,
    archived_rows (terminalize state store) = archived_rows store) /\
  (forall state store,
    active_row (terminalize state store) = None /\
    open_summary (terminalize state store) = None /\
    terminal_summary (terminalize state store) = Some state /\
    lifecycle_open (terminalize state store) = false /\
    lifecycle_terminal (terminalize state store) = Some state) /\
  (forall legacy_rows partial_rows,
    fresh_activation_allowed legacy_rows partial_rows = true ->
    legacy_rows = 0 /\ partial_rows = 0).
Proof.
  split.
  - apply legacy_and_v6_identities_are_disjoint.
  - split.
    + apply archive_insert_is_idempotent.
    + split.
      * apply canonical_rank_is_permutation_invariant.
      * split.
        -- apply terminalization_preserves_exact_archive.
        -- split.
           ++ apply terminalization_is_atomic_across_occurrence_and_lifecycle_state.
           ++ apply successful_activation_has_no_legacy_or_partial_rows.
Qed.

Print Assumptions deploy_occurrence_storage_contract.
Print Assumptions deploy_ingress_window_contract.
