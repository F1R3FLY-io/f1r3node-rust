(* G1-4a fix (DR-122, review M-3 and M-5): the compute row of the plan bounds
   the compute row of the vector.

   When rule X1 holds, the estimate takes its compute row from the static
   authority plan (static_authority_plan in delta_sigma.rs, assemble in
   delta_sigma/estimate.rs). Otherwise it takes the row of the vector fold.
   P1 def:token-demand makes the signed layers inside sent data demand,
   Delta_s(send x U) = Delta_s(U) (P1:2002-2027), and the plan also counts one
   unit for the lane of a signed body without a participant. The vector
   counts only the participants that a run introduces. ClosedDemandBound.v
   proves that the vector's row bounds the realized compute. This module
   proves that the plan's row is at least the vector's row for every lane,
   so the plan's row bounds the realized compute too.

   The model.
   - A planned term is a term of ClosedDemandBound.v with two additions that
     only the plan reads: each send or receive in process position carries
     the lanes of the signed layers inside its data, with multiplicity, and
     an empty signed body carries the lanes of its scope.
   - [erase] drops the additions. The result is the term that the vector
     reads.
   - [plan] follows the combination of the plan: parallel parts add, a
     receive adds its own count to the count of its body, and a branch point
     takes the maximum (the branch maxima of design section 5.5).

   The theorems.
   1. [plan_compute_dominates_vector]: for every lane, the plan's compute
      count of a planned term is at least the vector's compute count of its
      erasure.
   2. [plan_bounds_realized_compute]: with complete_estimate_sound, the
      plan's count bounds the realized compute of every run of the erasure.
   3. [plan_counts_data_layers]: the term of review M-3,
      `new x in { x!({% Nil %}[ alice ]) | for (_ <- x) { Nil } }`. The plan
      counts one unit of alice and the vector counts none, so the plan is an
      upper bound and not an equality.

   Negative control.
   - [nc_plan_without_own_counts_breaks_the_bound]: a plan that counts only
     the data layers and the empty bodies, and not the seal regions of the
     participants, stays below the realized compute of a run.

   Limits (second G1-4a fix, re-review MEDIUM-2).
   - [own_count] is defined as the vector's compute contribution of a
     participant, so [own_count_is_compute] holds by reflexivity, and
     [plan_compute_dominates_vector] shows only that the data layers and the
     empty bodies, which are counts of zero or more, keep a sum or a maximum
     at least as large.
   - The model does not follow the plan's walk of the chain of enclosing
     scopes (delta_sigma.rs:283-302), the deploy lane outside every scope,
     the receive with signed binds outside every scope, which has no deploy
     lane (delta_sigma.rs:487-516), or the unit lanes.
   - The model takes the data layers and the empty bodies as given lists. It
     does not model how the plan finds them in the syntax.
   - A Rust property test covers the code itself. On generated programs and
     on the corpus, wherever rule X1 holds, the plan's compute count is at
     least the vector's for every lane. The tests are
     the_plan_compute_row_dominates_the_vector_row and
     the_plan_compute_row_dominates_on_the_corpus in
     estimate/stack_safety_tests.rs.

   No axiom is used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Sorting.Permutation Lia.
Import ListNotations.
From CostAccountedRho Require Import ClosedDemandBound.

Inductive planned : Type :=
| PlanNil : planned
| PlanPar : planned -> planned -> planned
| PlanAct : participant -> list lane -> planned
| PlanRecv : participant -> list lane -> planned -> planned
| PlanIf : planned -> planned -> planned
| PlanEmpty : list lane -> planned.

Fixpoint erase (t : planned) : proc :=
  match t with
  | PlanNil => TNil
  | PlanPar first second => TPar (erase first) (erase second)
  | PlanAct p _ => TAct p
  | PlanRecv r _ body => TRecv r (erase body)
  | PlanIf first second => TIf (erase first) (erase second)
  | PlanEmpty _ => TNil
  end.

(** The count of a participant in the plan: one unit for each region of the
    seal of a send, or of the COMM authority of a receive. *)
Definition own_count (p : participant) (s : lane) : nat :=
  match p with
  | PSend regions _ _ _ => mu s regions
  | PRecv _ comm _ _ => mu s comm
  end.

Fixpoint plan (t : planned) (s : lane) : nat :=
  match t with
  | PlanNil => 0
  | PlanPar first second => plan first s + plan second s
  | PlanAct p layers => own_count p s + mu s layers
  | PlanRecv r layers body => own_count r s + mu s layers + plan body s
  | PlanIf first second => Nat.max (plan first s) (plan second s)
  | PlanEmpty lanes => mu s lanes
  end.

(** The vector's compute contribution of a participant is its count in the
    plan, for every table. *)
Lemma own_count_is_compute : forall all p s, contrib all p s 0 = own_count p s.
Proof. intros all [regions c m ib | intro comm chans ib] s; reflexivity. Qed.

Theorem plan_compute_dominates_vector : forall t all s,
  estimate all (erase t) s 0 <= plan t s.
Proof.
  induction t as [| first IHfirst second IHsecond | p layers | r layers body IH
                 | first IHfirst second IHsecond | lanes];
    intros all s; simpl.
  - lia.
  - specialize (IHfirst all s). specialize (IHsecond all s). lia.
  - rewrite own_count_is_compute. lia.
  - rewrite own_count_is_compute. specialize (IH all s). lia.
  - specialize (IHfirst all s). specialize (IHsecond all s). lia.
  - lia.
Qed.

Corollary plan_bounds_realized_compute : forall t xs comms s,
  selects (erase t) xs -> run_valid xs comms -> realized xs comms s 0 <= plan t s.
Proof.
  intros t xs comms s selection valid.
  apply Nat.le_trans with (estimate_of (erase t) s 0).
  - apply complete_estimate_sound; assumption.
  - unfold estimate_of. apply plan_compute_dominates_vector.
Qed.

(** The deploy lane 0, the lane 1 of alice and the channel 5. *)
Definition data_send : participant := PSend [0] 5 40 120.
Definition data_receive : participant := PRecv [0] [0] [5] 60.
Definition data_term : planned := PlanPar (PlanAct data_send [1]) (PlanRecv data_receive [] PlanNil).

Theorem plan_counts_data_layers :
  plan data_term 1 = 1 /\ estimate_of (erase data_term) 1 0 = 0.
Proof. split; vm_compute; reflexivity. Qed.

(** * Negative control *)

Fixpoint plan_layers_only (t : planned) (s : lane) : nat :=
  match t with
  | PlanNil => 0
  | PlanPar first second => plan_layers_only first s + plan_layers_only second s
  | PlanAct _ layers => mu s layers
  | PlanRecv _ layers body => mu s layers + plan_layers_only body s
  | PlanIf first second => Nat.max (plan_layers_only first s) (plan_layers_only second s)
  | PlanEmpty lanes => mu s lanes
  end.

Definition data_comm : comm := {|
  comm_receive := data_receive;
  comm_sends := [data_send];
  comm_authority := [0]
|}.

Theorem nc_plan_without_own_counts_breaks_the_bound :
  selects (erase data_term) [data_send; data_receive] /\
  run_valid [data_send; data_receive] [data_comm] /\
  realized [data_send; data_receive] [data_comm] 0 0 > plan_layers_only data_term 0.
Proof.
  split.
  - change (erase data_term) with (TPar (TAct data_send) (TRecv data_receive TNil)).
    change [data_send; data_receive] with ([data_send] ++ [data_receive]).
    repeat constructor.
  - split; [split |].
    + apply Forall_cons; [| apply Forall_nil].
      vm_compute. split; [apply Forall_cons; [exact I | apply Forall_nil] |].
      split; [reflexivity |].
      exists [0]. apply Permutation_refl.
    + exists []. vm_compute. apply perm_swap.
    + vm_compute. lia.
Qed.

Print Assumptions own_count_is_compute.
Print Assumptions plan_compute_dominates_vector.
Print Assumptions plan_bounds_realized_compute.
Print Assumptions plan_counts_data_layers.
Print Assumptions nc_plan_without_own_counts_breaks_the_bound.
