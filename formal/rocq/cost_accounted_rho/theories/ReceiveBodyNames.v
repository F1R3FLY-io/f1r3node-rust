(* G1-3 (DR-121): names created in a receive body depend on the matched datum.

   The reducer runs a receive body with Blake2b512Random::merge of the
   continuation's randomness and the randomness of each matched datum
   (dispatch.rs). A `new` in the body allocates its names from a split of that
   merged randomness. The funding resolver of G1-2 gave the body the split of
   the receive's own randomness. That rule does not read the datum.

   1. [receive_body_names_depend_on_datum]: if merging different data gives
      different states, and a name determines the state that allocated it,
      then two different data give the body two different names.
   2. [no_datum_independent_rule_is_sound]: any rule that computes a name from
      the continuation alone disagrees with the reducer on one of two
      different data. [parent_split_rule_is_refuted] instantiates it with the
      G1-2 rule.
   3. [two_datum_witness_refutes_parent_split]: a concrete instance, with no
      premise, in which the reducer gives two data two names and the G1-2 rule
      matches neither.
   4. [receive_bodies_allocate_nothing]: with receive bodies unresolved, the
      one-pass resolver of StackSafeLexicalResolver.v allocates no name in a
      receive body. It only substitutes the outer names, so the names that the
      body creates stay bound levels, which the analyzer treats as dynamic
      authority.
   5. [dynamic_machine_refines_one_pass_resolver] instantiates the machine
      theorem of StackSafeLexicalResolver.v with the flag off.
      [dynamic_machine_leaves_receive_body_names_bound] and
      [parent_split_machine_names_receive_body_binders] run the machine on one
      receive body. With the flag off, the signature keeps its bound level.
      With the flag on, the machine claims a name.

   The two premises of item 1 are Section hypotheses: the separation of data
   by the merge and the collision resistance of the allocation. They become
   ordinary premises of each theorem when the section closes, so no axiom is
   used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.
From CostAccountedRho Require Import StackSafeLexicalResolver.

Section Datum.

Variable Rand : Type.
Variable NameValue : Type.

(** [Blake2b512Random::merge] of the continuation's randomness and the data's
    randomness. *)
Variable merge : Rand -> list Rand -> Rand.

(** [util::evaluation_random]: the split for the term at an index of a count. *)
Variable split_at : Rand -> nat -> nat -> Rand.

(** The first name that a `new` allocates from a state. *)
Variable first_name : Rand -> NameValue.

Variable name_eq_dec : forall first second : NameValue, {first = second} + {first <> second}.

(** The name that the reducer gives the first binder of the `new` at [index]
    of [count] terms of a receive body that matched [datum]. *)
Definition runtime_body_name (continuation datum : Rand) (index count : nat) : NameValue :=
  first_name (split_at (merge continuation [datum]) index count).

(** The G1-2 rule: the split of the receive's own randomness. *)
Definition parent_split_name (continuation : Rand) (index count : nat) : NameValue :=
  first_name (split_at continuation index count).

Hypothesis merge_separates_data : forall continuation first_datum second_datum,
  first_datum <> second_datum -> merge continuation [first_datum] <> merge continuation [second_datum].

Hypothesis allocation_is_collision_free : forall first_state second_state index count,
  first_name (split_at first_state index count) = first_name (split_at second_state index count) ->
  first_state = second_state.

Theorem receive_body_names_depend_on_datum : forall continuation first_datum second_datum index count,
  first_datum <> second_datum ->
  runtime_body_name continuation first_datum index count <>
  runtime_body_name continuation second_datum index count.
Proof.
  intros continuation first_datum second_datum index count different same.
  unfold runtime_body_name in same.
  apply allocation_is_collision_free in same.
  exact (merge_separates_data continuation first_datum second_datum different same).
Qed.

(** A rule that reads only the continuation's randomness, the index and the
    count cannot give both data their names. *)
Theorem no_datum_independent_rule_is_sound :
  forall (rule : Rand -> nat -> nat -> NameValue) continuation first_datum second_datum index count,
  first_datum <> second_datum ->
  rule continuation index count <> runtime_body_name continuation first_datum index count \/
  rule continuation index count <> runtime_body_name continuation second_datum index count.
Proof.
  intros rule continuation first_datum second_datum index count different.
  destruct (name_eq_dec (rule continuation index count)
              (runtime_body_name continuation first_datum index count)) as [same | other].
  - right. intro same_second.
    apply (receive_body_names_depend_on_datum continuation first_datum second_datum index count
             different).
    rewrite <- same. exact same_second.
  - left. exact other.
Qed.

Corollary parent_split_rule_is_refuted :
  forall continuation first_datum second_datum index count,
  first_datum <> second_datum ->
  parent_split_name continuation index count <> runtime_body_name continuation first_datum index count \/
  parent_split_name continuation index count <> runtime_body_name continuation second_datum index count.
Proof.
  intros continuation first_datum second_datum index count different.
  exact (no_datum_independent_rule_is_sound parent_split_name continuation first_datum second_datum
           index count different).
Qed.

End Datum.

(** * A concrete two-datum witness *)

Fixpoint sum_of (values : list nat) : nat :=
  match values with
  | [] => 0
  | value :: rest => value + sum_of rest
  end.

Definition witness_merge (continuation : nat) (data : list nat) : nat :=
  continuation * 3 + 1 + 2 * sum_of data.

Definition witness_split (state index _ : nat) : nat := state * 31 + index.

Definition witness_first_name (state : nat) : nat := state.

Lemma witness_merge_separates_data : forall continuation first_datum second_datum,
  first_datum <> second_datum ->
  witness_merge continuation [first_datum] <> witness_merge continuation [second_datum].
Proof.
  intros continuation first_datum second_datum different same.
  unfold witness_merge in same. simpl in same. lia.
Qed.

Lemma witness_allocation_is_collision_free : forall first_state second_state index count,
  witness_first_name (witness_split first_state index count) =
  witness_first_name (witness_split second_state index count) ->
  first_state = second_state.
Proof.
  intros first_state second_state index count same.
  unfold witness_first_name, witness_split in same. lia.
Qed.

(** The witness satisfies both premises, so the general refutation applies to
    it. *)
Corollary witness_parent_split_rule_is_refuted :
  parent_split_name nat nat witness_split witness_first_name 0 0 1 <>
    runtime_body_name nat nat witness_merge witness_split witness_first_name 0 1 0 1 \/
  parent_split_name nat nat witness_split witness_first_name 0 0 1 <>
    runtime_body_name nat nat witness_merge witness_split witness_first_name 0 2 0 1.
Proof.
  apply (parent_split_rule_is_refuted nat nat witness_merge witness_split witness_first_name
           Nat.eq_dec witness_merge_separates_data witness_allocation_is_collision_free).
  discriminate.
Qed.

(** With the continuation 0 and the data 1 and 2, the reducer allocates the
    names 93 and 155 for the first `new` of a one-term body, and the G1-2 rule
    claims 0, which matches neither. *)
Theorem two_datum_witness_refutes_parent_split :
  runtime_body_name nat nat witness_merge witness_split witness_first_name 0 1 0 1 = 93 /\
  runtime_body_name nat nat witness_merge witness_split witness_first_name 0 2 0 1 = 155 /\
  parent_split_name nat nat witness_split witness_first_name 0 0 1 = 0.
Proof.
  repeat split; reflexivity.
Qed.

(** * The resolver leaves receive-body names unresolved *)

(** With receive bodies unresolved, the one-pass resolver handles a receive
    body as a substitution: it allocates no name, and it puts the outer names
    into the body. A `new` in the body adds holes, so the signatures that name
    its binders stay bound levels. *)
Theorem receive_bodies_allocate_nothing :
  forall (Seed : Type) (P : params Seed) e ts sigs count body,
  resolve_receive_bodies P = false ->
  r2_item P e ts (IReceive sigs count body) =
  Ok (IReceive (map (subst_sig e) sigs) count (subst P (holes count e) body)).
Proof.
  intros Seed P e ts sigs count body dynamic.
  assert (unresolved : receive_seed P ts = None).
  { unfold receive_seed. rewrite dynamic. reflexivity. }
  change (bind (r2 P (holes count e) (receive_seed P ts) body)
            (fun body' => Ok (IReceive (map (subst_sig e) sigs) count body')) =
          Ok (IReceive (map (subst_sig e) sigs) count (subst P (holes count e) body))).
  rewrite unresolved, (proj1 (r2_none P)). reflexivity.
Qed.

(** * The machine with receive bodies unresolved *)

(** The machine theorem of StackSafeLexicalResolver.v has no premise on
    [resolve_receive_bodies]. This instance turns the flag off, which is the
    rule of DR-121, so the theorem covers the G1-3 machine. *)
Definition dynamic_params : params nat := {|
  split_rand := fun seed index _ => seed * 7 + index + 1;
  allocate := fun spec seed => if 7 <=? spec then None else Some ([seed + 100], seed + 1);
  binders := fun _ => 1;
  resolve_receive_bodies := false
|}.

Corollary dynamic_machine_refines_one_pass_resolver : forall p seed,
  mrun dynamic_params (S (wsize p)) [TVisit p (Some seed)] ([], []) =
  bind (r2 dynamic_params no_names (Some seed) p) (fun p' => Ok ([], [VProc p'])).
Proof.
  intros p seed. apply machine_refines_one_pass_resolver.
Qed.

(** A receive whose body creates a name and signs a term with it. *)
Definition receive_body_witness : proc :=
  Proc (ICons (IReceive [] 0
          (Proc (ICons (INew 1 (Proc (ICons (ISigned (SBound 0) (Proc INil)) INil))) INil)))
        INil).

(** With the flag off, the signature keeps its bound level, which the analyzer
    treats as dynamic authority. *)
Theorem dynamic_machine_leaves_receive_body_names_bound :
  mrun dynamic_params (S (wsize receive_body_witness)) [TVisit receive_body_witness (Some 1)]
    ([], []) =
  Ok ([], [VProc receive_body_witness]).
Proof.
  vm_compute. reflexivity.
Qed.

(** With the flag on, the rule of G1-2, the machine claims the name 157 from
    the receive's own randomness. *)
Theorem parent_split_machine_names_receive_body_binders :
  mrun witness_params (S (wsize receive_body_witness)) [TVisit receive_body_witness (Some 1)]
    ([], []) =
  Ok ([], [VProc (Proc (ICons (IReceive [] 0
                   (Proc (ICons (INew 1 (Proc (ICons (ISigned (SName 157) (Proc INil)) INil)))
                            INil)))
                 INil))]).
Proof.
  vm_compute. reflexivity.
Qed.
