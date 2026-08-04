From Stdlib Require Import List Arith Bool FunctionalExtensionality.
From StackSafePDA Require Import StackSafePDA.

Import ListNotations.
Set Implicit Arguments.

(** A consensus-relevant abstraction of SpatialMatcher's stateful control
    operators.  [free_map] is the Rust [FreeMap]; a matcher value is a state
    transformer that may refuse.  Children are already-compiled matcher
    values, so this algebra states the exact snapshot discipline independently
    of either recursive Rust source or PDA scheduling. *)

Definition free_map := nat -> option nat.
Definition matcher_value := free_map -> option free_map.

Definition empty_free_map : free_map := fun _ => None.

Definition insert_binding
    (state : free_map) (level value : nat) : free_map :=
  fun candidate => if Nat.eq_dec candidate level then Some value else state candidate.

Definition bind_value
    (state : free_map) (level value : nat) : option free_map :=
  match state level with
  | None => Some (insert_binding state level value)
  | Some prior => if Nat.eqb prior value then Some state else None
  end.

Fixpoint sequence_children
    (children : list matcher_value) (state : free_map) : option free_map :=
  match children with
  | [] => Some state
  | child :: rest =>
      match child state with
      | None => None
      | Some next => sequence_children rest next
      end
  end.

(** Every disjunct receives the same [state].  A failed branch can therefore
    never leak a binding into the next attempt. *)
Fixpoint first_success
    (children : list matcher_value) (state : free_map) : option free_map :=
  match children with
  | [] => None
  | child :: rest =>
      match child state with
      | Some accepted => Some accepted
      | None => first_success rest state
      end
  end.

Inductive matcher_label : Type :=
| Exact : nat -> nat -> matcher_label
| Bind : nat -> nat -> matcher_label
| Sequence : matcher_label
| Conjunction : matcher_label
| Disjunction : matcher_label
| Negation : matcher_label.

Definition matcher_algebra
    (label : matcher_label) (children : list matcher_value) : matcher_value :=
  match label with
  | Exact target pattern =>
      fun state => if Nat.eqb target pattern then Some state else None
  | Bind level value => fun state => bind_value state level value
  | Sequence | Conjunction => fun state => sequence_children children state
  | Disjunction => fun state => first_success children state
  | Negation =>
      fun state =>
        match children with
        | child :: _ =>
            match child state with
            | Some _ => None
            | None => Some state
            end
        | [] => Some state
        end
  end.

Definition recursive_spatial_match
    (subject : @tree matcher_label) (initial : free_map) : option free_map :=
  @fold_tree matcher_label matcher_value matcher_algebra subject initial.

Definition pda_spatial_match
    (subject : @tree matcher_label) (initial : free_map) : option free_map :=
  match @run matcher_label matcher_value matcher_algebra
          (@compile_tree matcher_label subject) [] with
  | Some [compiled] => compiled initial
  | _ => None
  end.

Theorem spatial_match_pda_equivalent_to_recursive_match :
  forall subject initial,
    pda_spatial_match subject initial = recursive_spatial_match subject initial.
Proof.
  intros subject initial.
  unfold pda_spatial_match, recursive_spatial_match.
  rewrite (@pda_fold_equivalent_to_recursive_fold
    matcher_label matcher_value matcher_algebra subject).
  reflexivity.
Qed.

(** State-isolation lemmas used by the connective and retry frames. *)

Theorem failed_disjunct_retries_from_the_original_snapshot :
  forall first second state,
    first state = None ->
    first_success [first; second] state = second state.
Proof.
  intros first second state failed.
  simpl. rewrite failed. destruct (second state); reflexivity.
Qed.

Theorem successful_disjunct_commits_only_its_own_state :
  forall first second state accepted,
    first state = Some accepted ->
    first_success [first; second] state = Some accepted.
Proof.
  intros first second state accepted success.
  simpl. now rewrite success.
Qed.

Theorem successful_negation_restores_the_original_snapshot :
  forall child state,
    child state = None ->
    matcher_algebra Negation [child] state = Some state.
Proof.
  intros child state refused. simpl. now rewrite refused.
Qed.

Theorem refused_negation_discards_the_child_state :
  forall child state child_state,
    child state = Some child_state ->
    matcher_algebra Negation [child] state = None.
Proof.
  intros child state child_state accepted. simpl. now rewrite accepted.
Qed.

Theorem binding_is_idempotent_for_the_same_value :
  forall state level value,
    state level = Some value -> bind_value state level value = Some state.
Proof.
  intros state level value present.
  unfold bind_value. rewrite present, Nat.eqb_refl. reflexivity.
Qed.

Theorem binding_rejects_a_conflicting_value_without_mutation :
  forall state level prior value,
    state level = Some prior -> prior <> value -> bind_value state level value = None.
Proof.
  intros state level prior value present different.
  unfold bind_value. rewrite present.
  destruct (Nat.eqb prior value) eqn:equal; [|reflexivity].
  apply Nat.eqb_eq in equal. contradiction.
Qed.

(** The PathMap singleton optimization changes ownership, not semantics.  It
    removes the only target and pattern entry from their owned zippers and
    feeds the same pair to the matcher algebra. *)
Section SingletonPathMap.

Context {Entry : Type}.
Variable match_entry : Entry -> Entry -> matcher_value.

Definition recursive_singleton_path_match
    (target pattern : Entry) : matcher_value := match_entry target pattern.

Definition owned_zipper_singleton_path_match
    (target pattern : Entry) : matcher_value := match_entry target pattern.

Theorem owned_singleton_path_move_preserves_match_semantics :
  forall target pattern state,
    owned_zipper_singleton_path_match target pattern state =
    recursive_singleton_path_match target pattern state.
Proof. reflexivity. Qed.

End SingletonPathMap.
