From Stdlib Require Import Bool Arith List.
Import ListNotations.

Module DecoderOracleRouting.

Inductive destination := Frozen | Successor | Poisoned.
Record state := State { legacy : bool; poison : bool }.
Definition route (s : state) :=
  if legacy s then Frozen else if poison s then Poisoned else Successor.
Definition enter (s : state) := State true (poison s).
Definition arm (s : state) := State (legacy s) true.
Definition leave (saved current : state) := saved.

Theorem legacy_routes_frozen s : route (enter s) = Frozen.
Proof. reflexivity. Qed.
Theorem poisoned_legacy_routes_frozen s : route (arm (enter s)) = Frozen.
Proof. reflexivity. Qed.
Theorem poison_detects_successor : route (State false true) = Poisoned.
Proof. reflexivity. Qed.
Theorem ordinary_routes_successor : route (State false false) = Successor.
Proof. reflexivity. Qed.
Theorem scope_restores_exact_state s current : leave s current = s.
Proof. reflexivity. Qed.
Theorem nested_scope_restores_legacy s current :
  route (leave (enter s) current) = Frozen.
Proof. reflexivity. Qed.
Theorem routing_preserves_observation {A : Type} (frozen successor : A)
    (s : state) :
  (match route (enter s) with Frozen => Some frozen
   | Successor => Some successor | Poisoned => None end) = Some frozen.
Proof. reflexivity. Qed.
Theorem closed_reentry_stays_frozen s entries :
  map (fun (_ : nat) => route (enter s)) entries =
  repeat Frozen (length entries).
Proof. induction entries as [|a tail IH]; simpl; [reflexivity|now rewrite IH]. Qed.
Theorem disjoint_thread_state s (other : state) :
  snd (enter s, other) = other.
Proof. reflexivity. Qed.
Theorem missing_guard_poison_witness :
  route (arm (enter (State false false))) <>
  route (State false true).
Proof. discriminate. Qed.

Print Assumptions legacy_routes_frozen.
Print Assumptions poisoned_legacy_routes_frozen.
Print Assumptions poison_detects_successor.
Print Assumptions ordinary_routes_successor.
Print Assumptions scope_restores_exact_state.
Print Assumptions nested_scope_restores_legacy.
Print Assumptions routing_preserves_observation.
Print Assumptions closed_reentry_stays_frozen.
Print Assumptions disjoint_thread_state.
Print Assumptions missing_guard_poison_witness.

End DecoderOracleRouting.
