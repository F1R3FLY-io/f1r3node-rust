(* D-C5a (epic 8946, Phase D; decision record DR-100): the replay charge of
   a native operation does not depend on the path that it takes.

   A participant's operation either runs directly, when the participant is
   the only live one, or is submitted to the driver, which prepares every
   intent of the frontier and then executes them in operation order. Which
   path an operation takes depends on whether the other participants have
   completed, so on the thread schedule. Before DR-100 the driver path
   charged a joins read in [prepare] and the direct path did not, so the
   replays of one block were charged differently.

   An operation's own charge is a function of its position in operation
   order: native operations of one frontier run serialized (every native
   intent carries the footprint key [2, 0], so the frontier is one conflict
   component), and both paths execute the same operation with the same
   charges (measured: the direct and driver charges of a moved produce are
   equal).

   Results:
   - path_total_without_prepare_charge: with no prepare charge, the total of
     a run is the sum of the operation charges in operation order, whatever
     path each operation took.
   - direct_and_driver_paths_charge_alike: two runs of the same operations
     with any two path assignments are charged the same.
   - driver_footprint_read_charge_schedule_dependent: a negative control.
     With a prepare charge, a direct run and a driver run of one operation
     are charged differently.

   R1: the radix checkpoint's node cache assigns keys to shards with a
   per-instance random seed, and charged an insert by the population of its
   shard. Every insert is now charged the growth of a one-entry table:
   - constant_growth_dominates_shard_growth: if a table of k entries grows
     by at most k times the one-entry growth (true of hash_backing), the
     constant charge bounds the growth of the shards for every assignment.
   - constant_charge_independent_of_assignment: the constant charge depends
     only on the number of inserts.
   - shard_growth_depends_on_assignment: a negative control. Two keys in one
     shard grow by 260 bytes; in two shards, by 520.

   This complements DeterministicParallelReduction
   (direct_single_participant_refines_scheduled_execution), which proves
   that both paths compute the same result but says nothing about charges.

   Rust correspondence: rholang/src/rust/interpreter/deterministic_reduction.rs
   (ReductionSession::submit_produce and submit_consume, which choose the
   path; prepare, which no longer reads joins in native mode; drive). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive path := Direct | Driver.

Section Paths.
  (* The charge of the operation at a position in operation order, and the
     charge of preparing it on the driver path. *)
  Variables (operation_charge prepare_charge : nat -> nat).

  Fixpoint path_total (index : nat) (paths : list path) : nat :=
    match paths with
    | [] => 0
    | Direct :: rest => operation_charge index + path_total (S index) rest
    | Driver :: rest =>
        prepare_charge index + operation_charge index + path_total (S index) rest
    end.

  Fixpoint canonical_total (index count : nat) : nat :=
    match count with
    | 0 => 0
    | S remaining => operation_charge index + canonical_total (S index) remaining
    end.

  Theorem path_total_without_prepare_charge : forall paths index,
    (forall i, prepare_charge i = 0) ->
    path_total index paths = canonical_total index (length paths).
  Proof.
    induction paths as [| [|] rest IH]; intros index zero;
      cbn [path_total length canonical_total].
    - reflexivity.
    - rewrite (IH (S index) zero). reflexivity.
    - rewrite zero, (IH (S index) zero). reflexivity.
  Qed.

  Theorem direct_and_driver_paths_charge_alike : forall p1 p2 index,
    length p1 = length p2 ->
    (forall i, prepare_charge i = 0) ->
    path_total index p1 = path_total index p2.
  Proof.
    intros p1 p2 index same zero.
    rewrite (path_total_without_prepare_charge p1 index zero).
    rewrite (path_total_without_prepare_charge p2 index zero).
    rewrite same. reflexivity.
  Qed.
End Paths.

(* Negative control: with a joins read of charge 3 on the driver path, one
   operation of charge 5 is charged 5 directly and 8 through the driver. *)
Example driver_footprint_read_charge_schedule_dependent :
  path_total (fun _ => 5) (fun _ => 3) 0 [Direct] <>
    path_total (fun _ => 5) (fun _ => 3) 0 [Driver].
Proof. vm_compute. lia. Qed.

(* R1 (DR-100): the growth of the shards of a cache, given the number of
   keys of each shard. *)
Section ShardGrowth.
  (* The backing of a table of k entries. *)
  Variable growth : nat -> nat.
  Hypothesis growth_per_entry : forall k, growth k <= k * growth 1.

  Fixpoint shard_total (shards : list nat) : nat :=
    match shards with
    | [] => 0
    | k :: rest => growth k + shard_total rest
    end.

  Definition inserts (shards : list nat) : nat := fold_right Nat.add 0 shards.

  Theorem constant_growth_dominates_shard_growth : forall shards,
    shard_total shards <= inserts shards * growth 1.
  Proof.
    induction shards as [| k rest IH]; unfold inserts in *; cbn [shard_total fold_right].
    - lia.
    - rewrite Nat.mul_add_distr_r.
      pose proof (growth_per_entry k). lia.
  Qed.

  Theorem constant_charge_independent_of_assignment : forall first second,
    inserts first = inserts second ->
    inserts first * growth 1 = inserts second * growth 1.
  Proof. intros first second same. rewrite same. reflexivity. Qed.
End ShardGrowth.

(* Negative control: hash_backing of (ByteVector, Arc<Node>) is 260 bytes
   for one to three entries (four buckets) and 0 for none. *)
Definition small_table_growth (k : nat) : nat := if Nat.eqb k 0 then 0 else 260.

Example shard_growth_depends_on_assignment :
  shard_total small_table_growth [2] = 260 /\
  shard_total small_table_growth [1; 1] = 520.
Proof. split; reflexivity. Qed.

Print Assumptions path_total_without_prepare_charge.
Print Assumptions direct_and_driver_paths_charge_alike.
Print Assumptions driver_footprint_read_charge_schedule_dependent.
Print Assumptions constant_growth_dominates_shard_growth.
Print Assumptions constant_charge_independent_of_assignment.
Print Assumptions shard_growth_depends_on_assignment.
