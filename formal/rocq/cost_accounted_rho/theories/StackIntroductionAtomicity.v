From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.

Import ListNotations.

Record stack_introduction_state := {
  available_cells : nat;
  pending_cells : nat;
  committed_cells : nat;
  unrelated_committed_cells : nat;
  rspace_visible : bool;
  birth_visible : bool
}.

Definition physical_total (state : stack_introduction_state) : nat :=
  available_cells state + pending_cells state +
  committed_cells state + unrelated_committed_cells state.

Definition prepare_stack_introduction
  (cells : nat)
  (state : stack_introduction_state)
  : option stack_introduction_state :=
  if (0 <? cells) && (pending_cells state =? 0) &&
     (cells <=? available_cells state) then
    Some
      {|
        available_cells := available_cells state - cells;
        pending_cells := cells;
        committed_cells := committed_cells state;
        unrelated_committed_cells := unrelated_committed_cells state;
        rspace_visible := rspace_visible state;
        birth_visible := birth_visible state
      |}
  else None.

Definition abort_stack_introduction
  (state : stack_introduction_state)
  : stack_introduction_state :=
  {|
    available_cells := available_cells state + pending_cells state;
    pending_cells := 0;
    committed_cells := committed_cells state;
    unrelated_committed_cells := unrelated_committed_cells state;
    rspace_visible := rspace_visible state;
    birth_visible := birth_visible state
  |}.

Definition mark_stack_produce_visible
  (state : stack_introduction_state)
  : stack_introduction_state :=
  {|
    available_cells := available_cells state;
    pending_cells := pending_cells state;
    committed_cells := committed_cells state;
    unrelated_committed_cells := unrelated_committed_cells state;
    rspace_visible := true;
    birth_visible := birth_visible state
  |}.

Definition commit_stack_introduction
  (state : stack_introduction_state)
  : option stack_introduction_state :=
  if rspace_visible state && (0 <? pending_cells state) then
    Some
      {|
        available_cells := available_cells state;
        pending_cells := 0;
        committed_cells := committed_cells state + pending_cells state;
        unrelated_committed_cells := unrelated_committed_cells state;
        rspace_visible := true;
        birth_visible := true
      |}
  else None.

Definition rollback_committed_stack_introduction
  (state : stack_introduction_state)
  : stack_introduction_state :=
  {|
    available_cells := available_cells state + pending_cells state + committed_cells state;
    pending_cells := 0;
    committed_cells := 0;
    unrelated_committed_cells := unrelated_committed_cells state;
    rspace_visible := false;
    birth_visible := false
  |}.

Record deployment_accounting_state := {
  linear_state : stack_introduction_state;
  attempted_byte_units : nat
}.

Definition rollback_failed_deployment
  (state : deployment_accounting_state)
  : deployment_accounting_state :=
  {|
    linear_state := rollback_committed_stack_introduction (linear_state state);
    attempted_byte_units := attempted_byte_units state
  |}.

Theorem preparation_is_capacity_conserving_and_invisible :
  forall cells before prepared,
    prepare_stack_introduction cells before = Some prepared ->
    physical_total prepared = physical_total before /\
    committed_cells prepared = committed_cells before /\
    unrelated_committed_cells prepared = unrelated_committed_cells before /\
    rspace_visible prepared = rspace_visible before /\
    birth_visible prepared = birth_visible before.
Proof.
  intros cells before prepared step.
  unfold prepare_stack_introduction in step.
  destruct (0 <? cells) eqn:positive;
  destruct (pending_cells before =? 0) eqn:no_pending;
  destruct (cells <=? available_cells before) eqn:fits;
  simpl in step; try discriminate.
  apply Nat.eqb_eq in no_pending.
  apply Nat.leb_le in fits.
  inversion step; subst; clear step.
  repeat split; unfold physical_total; simpl; lia.
Qed.

Theorem byte_rejection_after_preparation_restores_exact_state :
  forall cells before prepared,
    prepare_stack_introduction cells before = Some prepared ->
    abort_stack_introduction prepared = before.
Proof.
  intros cells [available pending committed unrelated visible birth] prepared step.
  unfold prepare_stack_introduction in step.
  simpl in step.
  destruct (0 <? cells) eqn:positive;
  destruct (pending =? 0) eqn:no_pending;
  destruct (cells <=? available) eqn:fits;
  simpl in step; try discriminate.
  apply Nat.eqb_eq in no_pending.
  apply Nat.leb_le in fits.
  subst pending.
  inversion step; subst; clear step.
  unfold abort_stack_introduction.
  simpl.
  f_equal; lia.
Qed.

Theorem commit_after_visible_produce_is_complete_and_conserving :
  forall cells before prepared committed,
    prepare_stack_introduction cells before = Some prepared ->
    commit_stack_introduction (mark_stack_produce_visible prepared) = Some committed ->
    physical_total committed = physical_total before /\
    pending_cells committed = 0 /\
    committed_cells committed = committed_cells before + cells /\
    unrelated_committed_cells committed = unrelated_committed_cells before /\
    rspace_visible committed = true /\
    birth_visible committed = true.
Proof.
  intros cells before prepared committed prepare commit.
  unfold prepare_stack_introduction in prepare.
  destruct (0 <? cells) eqn:positive;
  destruct (pending_cells before =? 0) eqn:no_pending;
  destruct (cells <=? available_cells before) eqn:fits;
  simpl in prepare; try discriminate.
  assert (positive_bound : 0 < cells) by
    (apply Nat.ltb_lt; exact positive).
  apply Nat.eqb_eq in no_pending.
  apply Nat.leb_le in fits.
  inversion prepare; subst; clear prepare.
  unfold commit_stack_introduction, mark_stack_produce_visible in commit.
  simpl in commit.
  rewrite positive in commit.
  inversion commit; subst; clear commit.
  repeat split; unfold physical_total; simpl; lia.
Qed.

Theorem preparation_cannot_oversubscribe_capacity :
  forall cells state prepared,
    available_cells state < cells ->
    prepare_stack_introduction cells state <> Some prepared.
Proof.
  intros cells state prepared insufficient step.
  unfold prepare_stack_introduction in step.
  apply Nat.leb_gt in insufficient.
  rewrite insufficient, andb_false_r in step.
  discriminate.
Qed.

Theorem abort_preserves_unrelated_commit :
  forall state,
    unrelated_committed_cells (abort_stack_introduction state) =
    unrelated_committed_cells state.
Proof.
  reflexivity.
Qed.

Theorem enclosing_deploy_failure_restores_linear_capacity :
  forall state,
    physical_total (rollback_committed_stack_introduction state) =
      physical_total state /\
    pending_cells (rollback_committed_stack_introduction state) = 0 /\
    committed_cells (rollback_committed_stack_introduction state) = 0 /\
    unrelated_committed_cells (rollback_committed_stack_introduction state) =
      unrelated_committed_cells state /\
    rspace_visible (rollback_committed_stack_introduction state) = false /\
    birth_visible (rollback_committed_stack_introduction state) = false.
Proof.
  intros [available pending committed unrelated visible birth].
  repeat split; unfold physical_total, rollback_committed_stack_introduction; simpl; lia.
Qed.

Theorem enclosing_deploy_failure_preserves_attempted_byte_cost :
  forall state,
    attempted_byte_units (rollback_failed_deployment state) =
      attempted_byte_units state.
Proof.
  reflexivity.
Qed.

Inductive rspace_trace_event : Type :=
  | StandaloneProduce (produce_identity : nat)
  | MatchedComm (produce_identities : list nat) (comm_identity : nat)
  | StandaloneConsume (consume_identity : nat).

Inductive authority_trace_item : Type :=
  | ProducedAuthority (produce_identity : nat)
  | CommAuthority (comm_identity : nat).

Definition extract_authority_trace_event
  (event : rspace_trace_event)
  : list authority_trace_item :=
  match event with
  | StandaloneProduce produce_identity => [ProducedAuthority produce_identity]
  | MatchedComm produce_identities comm_identity =>
      map ProducedAuthority produce_identities ++ [CommAuthority comm_identity]
  | StandaloneConsume _ => []
  end.

Fixpoint extract_authority_trace
  (events : list rspace_trace_event)
  : list authority_trace_item :=
  match events with
  | [] => []
  | event :: rest =>
      extract_authority_trace_event event ++ extract_authority_trace rest
  end.

Theorem every_matched_produce_is_causally_extracted :
  forall produces comm produce,
    In produce produces ->
    In (ProducedAuthority produce)
      (extract_authority_trace [MatchedComm produces comm]).
Proof.
  intros produces comm produce present.
  simpl.
  rewrite !in_app_iff.
  left.
  left.
  apply in_map_iff.
  exists produce.
  split; [reflexivity | exact present].
Qed.

Theorem matched_produces_precede_their_comm :
  forall produces comm,
    extract_authority_trace [MatchedComm produces comm] =
      map ProducedAuthority produces ++ [CommAuthority comm].
Proof.
  intros produces comm.
  simpl.
  rewrite app_nil_r.
  reflexivity.
Qed.

Print Assumptions preparation_is_capacity_conserving_and_invisible.
Print Assumptions byte_rejection_after_preparation_restores_exact_state.
Print Assumptions commit_after_visible_produce_is_complete_and_conserving.
Print Assumptions preparation_cannot_oversubscribe_capacity.
Print Assumptions abort_preserves_unrelated_commit.
Print Assumptions enclosing_deploy_failure_restores_linear_capacity.
Print Assumptions enclosing_deploy_failure_preserves_attempted_byte_cost.
Print Assumptions every_matched_produce_is_causally_extracted.
Print Assumptions matched_produces_precede_their_comm.
