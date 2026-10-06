From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingParentPath.
Import ListNotations.

Record discovery_state := {
  discovery_queue : list nat;
  discovery_known : nat -> bool;
  discovery_parent : nat -> nat;
  discovery_rank : nat -> nat
}.

Definition discovery_valid root edge state : Prop :=
  NoDup (discovery_queue state) /\
  discovery_known state root = true /\
  (forall node, discovery_known state node = true <-> In node (discovery_queue state)) /\
  (forall node, discovery_known state node = true ->
    discovery_rank state node < length (discovery_queue state) /\
    nth (discovery_rank state node) (discovery_queue state) root = node) /\
  (forall node, discovery_known state node = true -> node <> root ->
    discovery_known state (discovery_parent state node) = true /\
    discovery_rank state (discovery_parent state node) < discovery_rank state node /\
    edge (discovery_parent state node) node = true).

Definition discovery_initial root := {|
  discovery_queue := [root];
  discovery_known := fun node => Nat.eqb node root;
  discovery_parent := fun _ => root;
  discovery_rank := fun _ => 0
|}.

Definition discovery_insert state source target := {|
  discovery_queue := discovery_queue state ++ [target];
  discovery_known := fun node => if Nat.eqb node target then true else discovery_known state node;
  discovery_parent := fun node => if Nat.eqb node target then source else discovery_parent state node;
  discovery_rank := fun node => if Nat.eqb node target then length (discovery_queue state) else discovery_rank state node
|}.

Definition discovery_step edge state source target :=
  if discovery_known state source && edge source target && negb (discovery_known state target)
  then discovery_insert state source target else state.

Fixpoint discovery_history edge state operations :=
  match operations with
  | [] => state
  | (source, target) :: rest => discovery_history edge (discovery_step edge state source target) rest
  end.

Theorem discovery_initial_valid : forall root edge,
  discovery_valid root edge (discovery_initial root).
Proof.
  intros root edge. unfold discovery_valid, discovery_initial; simpl.
  split; [constructor; [intro absent; contradiction|constructor]|].
  split; [apply Nat.eqb_refl|]. split.
  - intros node. rewrite Nat.eqb_eq. intuition congruence.
  - split.
    + intros node same. apply Nat.eqb_eq in same. subst node. auto.
    + intros node same different. apply Nat.eqb_eq in same. contradiction.
Qed.

Theorem discovery_insert_preserves_validity : forall root edge state source target,
  discovery_valid root edge state ->
  discovery_known state source = true -> discovery_known state target = false ->
  edge source target = true ->
  discovery_valid root edge (discovery_insert state source target).
Proof.
  intros root edge state source target [distinct [root_known [membership [ranks parents]]]] source_known target_unknown linked.
  assert (target_absent : ~ In target (discovery_queue state)).
  { intro included. apply membership in included. congruence. }
  assert (root_different : root <> target) by (intro same; subst target; congruence).
  assert (source_different : source <> target) by (intro same; subst target; congruence).
  assert (known_different : forall node, discovery_known state node = true -> node <> target).
  { intros node known same. subst node. congruence. }
  unfold discovery_valid, discovery_insert; simpl.
  split.
  - apply NoDup_app; [exact distinct|constructor; [intro absent; contradiction|constructor]|].
    intros node included [same|absent]; [subst node; contradiction|contradiction].
  - split.
    + apply Nat.eqb_neq in root_different. now rewrite root_different.
    + split.
      * intros node. rewrite in_app_iff. simpl.
        destruct (Nat.eqb_spec node target); subst; intuition.
        -- left. now apply membership.
        -- now apply membership.
      * split.
        -- intros node known. destruct (Nat.eqb_spec node target) as [same|different].
           ++ subst node. rewrite length_app. simpl. split; [lia|].
              rewrite app_nth2 by lia. now rewrite Nat.sub_diag.
           ++ destruct (ranks node known) as [bounded positioned].
              rewrite length_app. simpl. split; [lia|].
              rewrite app_nth1 by exact bounded. exact positioned.
        -- intros node known nonroot. destruct (Nat.eqb_spec node target) as [same|different].
           ++ subst node. apply Nat.eqb_neq in source_different. rewrite source_different.
              destruct (ranks source source_known) as [bounded positioned]. auto.
           ++ destruct (parents node known nonroot) as [parent_known [earlier parent_edge]].
              specialize (known_different (discovery_parent state node) parent_known).
              apply Nat.eqb_neq in known_different. rewrite known_different. auto.
Qed.

Theorem discovery_step_preserves_validity : forall root edge state source target,
  discovery_valid root edge state ->
  discovery_valid root edge (discovery_step edge state source target).
Proof.
  intros root edge state source target valid. unfold discovery_step.
  destruct (discovery_known state source && edge source target && negb (discovery_known state target)) eqn:enabled; [|exact valid].
  apply andb_true_iff in enabled. destruct enabled as [first absent].
  apply andb_true_iff in first. destruct first as [known linked].
  apply negb_true_iff in absent. now apply discovery_insert_preserves_validity.
Qed.

Theorem arbitrary_discovery_histories_preserve_validity : forall operations root edge state,
  discovery_valid root edge state ->
  discovery_valid root edge (discovery_history edge state operations).
Proof.
  induction operations as [|[source target] rest IH]; intros root edge state valid; simpl; [exact valid|].
  apply IH. now apply discovery_step_preserves_validity.
Qed.

Theorem discovered_nodes_have_simple_root_paths : forall operations root edge node,
  let state := discovery_history edge (discovery_initial root) operations in
  discovery_known state node = true ->
  exists path,
    extract_ranked_parent_path (length (discovery_queue state)) root (discovery_parent state) node = Some path /\
    ranked_parent_path_certificate root (discovery_known state) (discovery_rank state) edge node path.
Proof.
  intros operations root edge node state known.
  assert (valid : discovery_valid root edge state).
  { apply arbitrary_discovery_histories_preserve_validity. apply discovery_initial_valid. }
  destruct valid as [_ [_ [_ [ranks parents]]]].
  destruct (ranks node known) as [bounded positioned].
  eapply strictly_ranked_parents_extract_a_simple_path; [exact parents|exact known|lia].
Qed.

Print Assumptions discovery_initial_valid.
Print Assumptions discovery_insert_preserves_validity.
Print Assumptions arbitrary_discovery_histories_preserve_validity.
Print Assumptions discovered_nodes_have_simple_root_paths.
