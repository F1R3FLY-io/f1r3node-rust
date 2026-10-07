(* D-D3 (epic 8946, Phase D; decision record DR-105): a pattern that is one
   free variable binds the target's fields directly.

   The general path matches a target against a pattern @x by ten calls of
   list_match_single_, one for each field list of a Par, in the order of the
   Par/Par matcher: sends, receives, news, exprs, matches, bundles,
   unforgeables, conditionals, cost_signed_terms, cost_stacks. For @x every
   pattern list is empty (the free-variable filter removes the expression x),
   so each call takes the remainder branch. It checks every target element of
   the field and, if all are closed, merges the field into the binding at the
   variable's level (FreeMapBindings part 2). The first field with an open
   element stops the match, and the fields before it stay merged. Callers can
   observe that partial binding, so the fast path must reproduce it.

   The fast path runs the same check and the same merge for each field in the
   same order. This file models both paths over an abstract free map. A
   binding holds the ten field lists and a shell that the merge never writes.

   Results:
   - fast_path_equals_general: the same result and the same free map,
     failure state included.
   - fast_path_failure_state: when the first open field follows the fields
     [prefix], the map holds the merges of [prefix].
   - fast_path_keeps_other_levels: no other level changes.
   - fast_fields_in_index_order: the fast path visits the fields in the order
     of their indices.
   Negative controls:
   - check_all_first_loses_partial_binding: checking every field before the
     first merge leaves another failure state.
   - field_order_changes_failure_state: the struct order, which puts
     unforgeables (field 6) before bundles (field 5), leaves another failure
     state.
   - legacy_free_variable_charge_includes_pattern_copy (with
     legacy_free_variable_charge_example): the general path's charge exceeds
     the fast path's by at least the inspection of the target.

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence: rholang/src/rust/interpreter/matcher/spatial_matcher.rs
   (free_variable_level, bind_free_variable, bind_free_variable_by_reference)
   and fold_match.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
From CostAccountedRho Require FreeMapBindings.
Import ListNotations.

Section FastPath.
  Variable K E R : Type.
  Variable key_eq_dec : forall a b : K, {a = b} + {a <> b}.
  Variable closed : E -> bool.
  Variable empty_rest : R.

  Record binding := { fields : list (list E); shell : R }.

  Definition fresh : binding := {| fields := repeat [] 10; shell := empty_rest |}.

  Fixpoint replace (k : nat) (x : list E) (xs : list (list E)) : list (list E) :=
    match k, xs with
    | 0, _ :: rest => x :: rest
    | S k', y :: rest => y :: replace k' x rest
    | _, [] => []
    end.

  Definition set_field (k : nat) (x : list E) (b : binding) : binding :=
    {| fields := replace k x (fields b); shell := shell b |}.

  Definition free_map := K -> option binding.

  (* The merge of FreeMapBindings part 2: in place on an existing binding, or
     a new binding inserted after its merge. *)
  Definition merge (level : K) (k : nat) (x : list E) (m : free_map) : free_map :=
    FreeMapBindings.in_place_merge K binding key_eq_dec
      [FreeMapBindings.Assign (set_field k x)] true m level fresh.

  (* list_match_single_ with remainder [level] and an empty pattern list. *)
  Definition remainder_field (level : K) (k : nat) (targets : list E) (m : free_map)
    : bool * free_map :=
    if forallb closed targets then (true, merge level k targets m) else (false, m).

  (* The branch for a nonempty pattern list is outside this file. *)
  Variable nonempty_branch : K -> nat -> list E -> list E -> free_map -> bool * free_map.

  Definition list_match_single (level : K) (k : nat) (targets patterns : list E)
    (m : free_map) : bool * free_map :=
    if length targets <? length patterns then (false, m)
    else match patterns with
         | [] => remainder_field level k targets m
         | _ :: _ => nonempty_branch level k targets patterns m
         end.

  Fixpoint general_fields (level : K) (k : nat) (targets patterns : list (list E))
    (m : free_map) : bool * free_map :=
    match targets, patterns with
    | t :: ts, p :: ps =>
        let '(accepted, m') := list_match_single level k t p m in
        if accepted then general_fields level (S k) ts ps m' else (false, m')
    | _, _ => (true, m)
    end.

  Fixpoint fast_fields (level : K) (k : nat) (targets : list (list E)) (m : free_map)
    : bool * free_map :=
    match targets with
    | t :: ts =>
        if forallb closed t then fast_fields level (S k) ts (merge level k t m)
        else (false, m)
    | [] => (true, m)
    end.

  Theorem fast_path_equals_general : forall targets patterns level k m,
    length patterns = length targets ->
    Forall (fun p => p = []) patterns ->
    fast_fields level k targets m = general_fields level k targets patterns m.
  Proof.
    induction targets as [| t ts IH]; intros patterns level k m Hlength Hempty.
    - destruct patterns; reflexivity.
    - destruct patterns as [| p ps]; [discriminate |].
      inversion Hempty as [| ? ? Hp Hps]; subst p.
      unfold general_fields; fold general_fields.
      unfold list_match_single, remainder_field. simpl.
      destruct (forallb closed t); [| reflexivity].
      apply IH; [simpl in Hlength; lia | exact Hps].
  Qed.

  Fixpoint merges (level : K) (k : nat) (targets : list (list E)) (m : free_map)
    : free_map :=
    match targets with
    | t :: ts => merges level (S k) ts (merge level k t m)
    | [] => m
    end.

  Theorem fast_path_failure_state : forall prefix t suffix level k m,
    Forall (fun x => forallb closed x = true) prefix ->
    forallb closed t = false ->
    fast_fields level k (prefix ++ t :: suffix) m = (false, merges level k prefix m).
  Proof.
    induction prefix as [| x prefix IH]; intros t suffix level k m Hprefix Hopen; simpl.
    - rewrite Hopen. reflexivity.
    - inversion Hprefix as [| ? ? Hx Hrest]; subst. rewrite Hx. apply IH; assumption.
  Qed.

  Lemma merge_keeps_other_levels : forall level k x m other,
    level <> other -> merge level k x m other = m other.
  Proof.
    intros level k x m other Hdiff.
    unfold merge, FreeMapBindings.in_place_merge, FreeMapBindings.update_map.
    destruct (m level) as [value |].
    - destruct (key_eq_dec level other); [contradiction | reflexivity].
    - simpl. destruct (key_eq_dec level other); [contradiction | reflexivity].
  Qed.

  Theorem fast_path_keeps_other_levels : forall targets level k m other,
    level <> other -> snd (fast_fields level k targets m) other = m other.
  Proof.
    induction targets as [| t ts IH]; intros level k m other Hdiff; simpl; [reflexivity |].
    destruct (forallb closed t); simpl; [| reflexivity].
    rewrite IH by assumption. apply merge_keeps_other_levels. exact Hdiff.
  Qed.

  (* The fast path over explicit (field index, field) pairs. *)
  Fixpoint fast_indexed (level : K) (fields_in_order : list (nat * list E)) (m : free_map)
    : bool * free_map :=
    match fields_in_order with
    | (k, t) :: rest =>
        if forallb closed t then fast_indexed level rest (merge level k t m) else (false, m)
    | [] => (true, m)
    end.

  Lemma fast_fields_in_index_order : forall targets level k m,
    fast_fields level k targets m
    = fast_indexed level (combine (seq k (length targets)) targets) m.
  Proof.
    induction targets as [| t ts IH]; intros level k m; simpl; [reflexivity |].
    destruct (forallb closed t); [apply IH | reflexivity].
  Qed.

  (* Negative control: checking every field before the first merge. *)
  Definition check_all_first (level : K) (k : nat) (targets : list (list E)) (m : free_map)
    : bool * free_map :=
    if forallb (forallb closed) targets then (true, merges level k targets m) else (false, m).

  Lemma fresh_merge_is_stored : forall level k x m,
    m level = None -> merge level k x m level <> None.
  Proof.
    intros level k x m Hnone.
    unfold merge, FreeMapBindings.in_place_merge, FreeMapBindings.update_map.
    rewrite Hnone. simpl.
    destruct (key_eq_dec level level); [discriminate | contradiction].
  Qed.

  Theorem check_all_first_loses_partial_binding : forall c o level m,
    closed c = true -> closed o = false -> m level = None ->
    snd (fast_fields level 0 [[c]; [o]] m) level
    <> snd (check_all_first level 0 [[c]; [o]] m) level.
  Proof.
    intros c o level m Hc Ho Hnone.
    unfold check_all_first. simpl. rewrite Hc, Ho. simpl.
    rewrite Hnone. apply fresh_merge_is_stored. exact Hnone.
  Qed.

  (* Negative control: the struct order merges unforgeables (field 6) before
     it checks bundles (field 5). *)
  Theorem field_order_changes_failure_state : forall c o level m,
    closed c = true -> closed o = false -> m level = None ->
    snd (fast_indexed level [(5, [o]); (6, [c])] m) level
    <> snd (fast_indexed level [(6, [c]); (5, [o])] m) level.
  Proof.
    intros c o level m Hc Ho Hnone. simpl. rewrite Hc, Ho. simpl.
    rewrite Hnone. intros Hsame. symmetry in Hsame.
    exact (fresh_merge_is_stored level 6 [c] m Hnone Hsame).
  Qed.
End FastPath.

(* Negative control: charges of one pair at the fold site, in scanned bytes.
   A flag read costs 1, a copy and its cleanup cost 2 s for a value of s
   bytes, and an inspection costs 2 s. The general path copies and inspects
   the target and the pattern; the fast path runs the free-variable test and
   copies the target's fields, which hold at most the target's bytes. *)
Definition legacy_pair_charge (target pattern work : nat) : nat :=
  1 + 2 * target + 2 * pattern + 1 + 2 * target + 2 * pattern + work.

Definition fast_pair_charge (target test work : nat) : nat :=
  1 + test + 2 * target + work.

Theorem legacy_free_variable_charge_includes_pattern_copy : forall target pattern test work,
  test <= 4 * pattern + 1 ->
  fast_pair_charge target test work + 2 * target <= legacy_pair_charge target pattern work.
Proof. intros target pattern test work Htest. unfold fast_pair_charge, legacy_pair_charge. lia. Qed.

Example legacy_free_variable_charge_example :
  legacy_pair_charge 1024 1 0 - fast_pair_charge 1024 5 0 = 2048.
Proof. reflexivity. Qed.
