From Stdlib Require Import Arith Bool Lists.List.
Import ListNotations.

Record buffer_rows := {
  explicit : nat -> bool;
  dependency : nat -> nat -> bool
}.

Definition well_formed (rows : buffer_rows) : Prop :=
  forall child parent, dependency rows child parent = true -> explicit rows child = true.

Definition rows_equivalent (left right : buffer_rows) : Prop :=
  (forall child, explicit left child = explicit right child) /\
  (forall child parent, dependency left child parent = dependency right child parent).

Definition ensure (rows : buffer_rows) (key : nat) : buffer_rows :=
  {| explicit := fun child => if Nat.eq_dec child key then true else explicit rows child;
     dependency := dependency rows |}.

Definition add (rows : buffer_rows) (parent key : nat) : buffer_rows :=
  {| explicit := fun child => if Nat.eq_dec child key then true else explicit rows child;
     dependency := fun child source =>
       if Nat.eq_dec child key then
         if Nat.eq_dec source parent then true else dependency rows child source
       else dependency rows child source |}.

Definition remove (rows : buffer_rows) (key : nat) : buffer_rows :=
  {| explicit := fun child => if Nat.eq_dec child key then false else explicit rows child;
     dependency := fun child source =>
       if Nat.eq_dec child key then false else
         if Nat.eq_dec source key then false else dependency rows child source |}.

Inductive mutation := Ensure (key : nat) | Add (parent child : nat) | Remove (key : nat).

Definition apply_mutation (rows : buffer_rows) (change : mutation) : buffer_rows :=
  match change with
  | Ensure key => ensure rows key
  | Add parent child => add rows parent child
  | Remove key => remove rows key
  end.

Definition transact (rows : buffer_rows) (change : mutation) (commits : bool) : buffer_rows :=
  if commits then apply_mutation rows change else rows.

Fixpoint history (rows : buffer_rows) (changes : list (mutation * bool)) : buffer_rows :=
  match changes with
  | [] => rows
  | (change, commits) :: rest => history (transact rows change commits) rest
  end.

Theorem ensure_preserves_all_dependencies : forall rows key child parent,
  dependency (ensure rows key) child parent = dependency rows child parent.
Proof. reflexivity. Qed.

Theorem ensure_preserves_well_formedness : forall rows key,
  well_formed rows -> well_formed (ensure rows key).
Proof.
  intros rows key H child parent E. simpl in *.
  destruct (Nat.eq_dec child key); auto. now apply H with parent.
Qed.

Theorem add_preserves_well_formedness : forall rows parent key,
  well_formed rows -> well_formed (add rows parent key).
Proof.
  intros rows parent key H child source E. simpl in *.
  destruct (Nat.eq_dec child key); auto. now apply H with source.
Qed.

Theorem remove_preserves_well_formedness : forall rows key,
  well_formed rows -> well_formed (remove rows key).
Proof.
  intros rows key H child parent E. simpl in *.
  destruct (Nat.eq_dec child key); try discriminate.
  destruct (Nat.eq_dec parent key); try discriminate. now apply H with parent.
Qed.

Theorem remove_preserves_every_other_explicit_identity : forall rows key child,
  child <> key -> explicit (remove rows key) child = explicit rows child.
Proof. intros. simpl. destruct (Nat.eq_dec child key); congruence. Qed.

Theorem remove_preserves_every_unrelated_dependency : forall rows key child parent,
  child <> key -> parent <> key ->
  dependency (remove rows key) child parent = dependency rows child parent.
Proof.
  intros. simpl. destruct (Nat.eq_dec child key); try congruence.
  destruct (Nat.eq_dec parent key); congruence.
Qed.

Theorem remove_erases_all_incoming_and_outgoing_edges : forall rows key other,
  dependency (remove rows key) key other = false /\
  dependency (remove rows key) other key = false.
Proof.
  intros. simpl. destruct (Nat.eq_dec key key); try congruence.
  destruct (Nat.eq_dec other key); auto.
Qed.

Theorem ready_child_remains_explicit : forall rows key child,
  child <> key -> explicit rows child = true ->
  (forall parent, dependency rows child parent = true -> parent = key) ->
  explicit (remove rows key) child = true /\
  (forall parent, dependency (remove rows key) child parent = false).
Proof.
  intros rows key child Hneq Hown Honly. split.
  - now rewrite remove_preserves_every_other_explicit_identity.
  - intro parent. simpl. destruct (Nat.eq_dec child key); try congruence.
    destruct (Nat.eq_dec parent key); auto.
    destruct (dependency rows child parent) eqn:E; auto.
    specialize (Honly parent E). congruence.
Qed.

Theorem ensure_is_idempotent : forall rows key,
  rows_equivalent (ensure (ensure rows key) key) (ensure rows key).
Proof.
  intros. split; intros; simpl; try reflexivity.
  destruct (Nat.eq_dec child key); reflexivity.
Qed.

Theorem remove_is_idempotent : forall rows key,
  rows_equivalent (remove (remove rows key) key) (remove rows key).
Proof.
  intros. split; intros; simpl;
    destruct (Nat.eq_dec child key); try reflexivity.
  destruct (Nat.eq_dec parent key); reflexivity.
Qed.

Theorem removals_commute : forall rows first second,
  rows_equivalent (remove (remove rows first) second) (remove (remove rows second) first).
Proof.
  intros. split; intros; simpl.
  all: repeat match goal with
       | |- context [Nat.eq_dec ?left ?right] => destruct (Nat.eq_dec left right)
       end; reflexivity.
Qed.

Theorem failed_transaction_preserves_every_row : forall rows change,
  transact rows change false = rows.
Proof. reflexivity. Qed.

Theorem committed_transaction_has_exact_requested_effect : forall rows change,
  transact rows change true = apply_mutation rows change.
Proof. reflexivity. Qed.

Theorem transaction_preserves_well_formedness : forall rows change commits,
  well_formed rows -> well_formed (transact rows change commits).
Proof.
  intros rows change commits H. unfold transact. destruct commits; auto.
  destruct change; simpl.
  - now apply ensure_preserves_well_formedness.
  - now apply add_preserves_well_formedness.
  - now apply remove_preserves_well_formedness.
Qed.

Theorem all_finite_transaction_histories_preserve_well_formedness : forall changes rows,
  well_formed rows -> well_formed (history rows changes).
Proof.
  induction changes as [|[change commits] rest IH]; intros rows H; simpl; auto.
  apply IH. now apply transaction_preserves_well_formedness.
Qed.

Print Assumptions ensure_preserves_all_dependencies.
Print Assumptions ensure_preserves_well_formedness.
Print Assumptions add_preserves_well_formedness.
Print Assumptions remove_preserves_well_formedness.
Print Assumptions remove_preserves_every_other_explicit_identity.
Print Assumptions remove_preserves_every_unrelated_dependency.
Print Assumptions remove_erases_all_incoming_and_outgoing_edges.
Print Assumptions ready_child_remains_explicit.
Print Assumptions ensure_is_idempotent.
Print Assumptions remove_is_idempotent.
Print Assumptions removals_commute.
Print Assumptions failed_transaction_preserves_every_row.
Print Assumptions committed_transaction_has_exact_requested_effect.
Print Assumptions transaction_preserves_well_formedness.
Print Assumptions all_finite_transaction_histories_preserve_well_formedness.
