From Stdlib Require Import List Arith Bool Lia.
Import ListNotations.

Module ProtobufParAccumulation.

Record observed := Observe {
  components : list nat;
  locally_free : list nat;
  connective_used : bool;
  position : nat
}.
Record planned := Plan {
  roots : list (list nat);
  root : nat;
  metadata : list nat;
  connective : bool;
  cursor : nat
}.
Definition lookup (a : list (list nat)) (i : nat) := nth i a [].
Definition observe p := Observe (lookup (roots p) (root p))
  (metadata p) (connective p) (cursor p).

Inductive action :=
| Insert (key : nat)
| SetLocallyFree (bytes : list nat)
| SetConnective (flag : bool)
| Refuse (error : nat).
Record event := Event { update : action; consumed : nat }.

Definition reference_step (maximum : nat) (s : observed) (e : event)
    : observed * option nat :=
  let offset := position s + consumed e in
  match update e with
  | Insert key =>
      if count_occ Nat.eq_dec (components s) key <? maximum then
        (Observe (key :: components s) (locally_free s) (connective_used s) offset, None)
      else (Observe (components s) (locally_free s) (connective_used s) offset, Some 0)
  | SetLocallyFree bytes =>
      (Observe (components s) bytes (connective_used s) offset, None)
  | SetConnective flag => (Observe (components s) (locally_free s) flag offset, None)
  | Refuse error =>
      (Observe (components s) (locally_free s) (connective_used s) offset, Some error)
  end.

Definition planned_step (maximum : nat) (s : planned) (e : event)
    : planned * option nat :=
  let offset := cursor s + consumed e in
  match update e with
  | Insert key =>
      let old := lookup (roots s) (root s) in
      if count_occ Nat.eq_dec old key <? maximum then
        (Plan (roots s ++ [key :: old]) (length (roots s))
          (metadata s) (connective s) offset, None)
      else (Plan (roots s) (root s) (metadata s) (connective s) offset, Some 0)
  | SetLocallyFree bytes =>
      (Plan (roots s) (root s) bytes (connective s) offset, None)
  | SetConnective flag => (Plan (roots s) (root s) (metadata s) flag offset, None)
  | Refuse error =>
      (Plan (roots s) (root s) (metadata s) (connective s) offset, Some error)
  end.

Lemma lookup_appended a value : lookup (a ++ [value]) (length a) = value.
Proof. unfold lookup. rewrite nth_middle. reflexivity. Qed.

Theorem each_field_refines maximum s e :
  let '(next, error) := planned_step maximum s e in
  (observe next, error) = reference_step maximum (observe s) e.
Proof.
  destruct s, e as [action bytes], action; cbn; try reflexivity.
  match goal with |- context [if ?test then _ else _] => destruct test end;
    cbn; try reflexivity. unfold observe, lookup; cbn. now rewrite nth_middle.
Qed.

Fixpoint execute {S : Type} (step : S -> event -> S * option nat)
    (events : list event) (s : S) : S * option nat * list event :=
  match events with
  | [] => (s, None, [])
  | e :: tail =>
      let '(next, error) := step s e in
      match error with
      | Some code => (next, Some code, tail)
      | None => execute step tail next
      end
  end.

Definition observation (result : planned * option nat * list event) :=
  let '(s, error, tail) := result in (observe s, error, tail).

Theorem ordered_fields_refine maximum events s :
  observation (execute (planned_step maximum) events s) =
  execute (reference_step maximum) events (observe s).
Proof.
  revert s. induction events as [|e tail IH]; intros s; cbn; [reflexivity|].
  pose proof (each_field_refines maximum s e) as correspondence.
  destruct (planned_step maximum s e) as [next error] eqn:step.
  rewrite <- correspondence. destruct error; cbn; [reflexivity|apply IH].
Qed.

Theorem earlier_arena_roots_survive a suffix i :
  i < length a -> lookup (a ++ suffix) i = lookup a i.
Proof. intros bounded. unfold lookup. now rewrite app_nth1. Qed.

Theorem insertion_preserves_metadata maximum s key consumed :
  let '(next, _) := planned_step maximum s (Event (Insert key) consumed) in
  metadata next = metadata s /\ connective next = connective s.
Proof.
  cbn. match goal with |- context [if ?test then _ else _] => destruct test end; auto.
Qed.

Theorem metadata_update_preserves_authority maximum s bytes consumed :
  let '(next, _) := planned_step maximum s (Event (SetLocallyFree bytes) consumed) in
  roots next = roots s /\ root next = root s.
Proof. cbn; auto. Qed.

Theorem connective_update_preserves_authority maximum s flag consumed :
  let '(next, _) := planned_step maximum s (Event (SetConnective flag) consumed) in
  roots next = roots s /\ root next = root s.
Proof. cbn; auto. Qed.

Theorem explicit_empty_locally_free_replaces maximum s consumed :
  metadata (fst (planned_step maximum s (Event (SetLocallyFree []) consumed))) = [].
Proof. reflexivity. Qed.

Theorem explicit_false_connective_replaces maximum s consumed :
  connective (fst (planned_step maximum s (Event (SetConnective false) consumed))) = false.
Proof. reflexivity. Qed.

Theorem insertion_failure_preserves_authority maximum s key consumed :
  maximum <= count_occ Nat.eq_dec (lookup (roots s) (root s)) key ->
  let '(next, error) := planned_step maximum s (Event (Insert key) consumed) in
  roots next = roots s /\ root next = root s /\ metadata next = metadata s /\
  connective next = connective s /\ error = Some 0.
Proof.
  intros limit.
  assert ((count_occ Nat.eq_dec (lookup (roots s) (root s)) key <? maximum) = false)
    as full by (apply Nat.ltb_ge; exact limit).
  unfold planned_step. cbn -[Nat.ltb lookup]. rewrite full.
  cbn; repeat split; reflexivity.
Qed.

Theorem refusal_preserves_committed_state maximum s error consumed :
  let '(next, failure) := planned_step maximum s (Event (Refuse error) consumed) in
  roots next = roots s /\ root next = root s /\ metadata next = metadata s /\
  connective next = connective s /\ cursor next = cursor s + consumed /\
  failure = Some error.
Proof. cbn; repeat split; reflexivity. Qed.

Theorem refusal_does_not_visit_suffix maximum s error consumed tail :
  execute (planned_step maximum) (Event (Refuse error) consumed :: tail) s =
  (Plan (roots s) (root s) (metadata s) (connective s) (cursor s + consumed), Some error, tail).
Proof. reflexivity. Qed.

Theorem derived_metadata_mutant_changes_observation :
  Observe [1] [] false 1 <> Observe [1] [7] true 1.
Proof. discriminate. Qed.

Theorem union_metadata_mutant_changes_explicit_reset :
  metadata (fst (planned_step 5 (Plan [[]] 0 [7] true 0)
    (Event (SetLocallyFree []) 1))) <> [7].
Proof. discriminate. Qed.

Print Assumptions each_field_refines.
Print Assumptions ordered_fields_refine.
Print Assumptions earlier_arena_roots_survive.
Print Assumptions insertion_preserves_metadata.
Print Assumptions metadata_update_preserves_authority.
Print Assumptions connective_update_preserves_authority.
Print Assumptions explicit_empty_locally_free_replaces.
Print Assumptions explicit_false_connective_replaces.
Print Assumptions insertion_failure_preserves_authority.
Print Assumptions refusal_preserves_committed_state.
Print Assumptions refusal_does_not_visit_suffix.
Print Assumptions derived_metadata_mutant_changes_observation.
Print Assumptions union_metadata_mutant_changes_explicit_reset.

End ProtobufParAccumulation.
