(* D-O5 (epic 8946, Phase D item D-A2; decision record DR-89): the replay
   authority prepare charges only the reads and copies of the branch that a
   COMM row takes.

   The model reuses the reservation and read events of
   ObservationReadCoverage. Replay authority prepares each recorded COMM row
   in one of three branches:
   - Retry: the row repeats a granted event; prepare walks the saved and the
     recorded observation and compares them (a read of both).
   - Granted: a new granted event; prepare computes the self-metered demand
     and copies the authority into the event.
   - Frontier: a new event that is not granted; publication copies the
     authority into the frontier.
   Before D-O5, prepare walked the whole recorded observation and reserved
   the authority copy for every row, whatever the branch.

   Results:
   - prepare_trace_covered and legacy_prepare_trace_covered: every branch
     satisfies prefix coverage, before and after;
   - prepare_reads_equal_legacy_reads: every branch performs the same reads;
   - legacy_prepare_excess: the legacy prepare reserved exactly the
     observation walk more (granted and frontier branches) or the authority
     copy more (retry branch);
   - prepare_charge_independent_of_unread_values: the granted and frontier
     charges do not depend on the observation, the retry charge does not
     depend on the authority;
   - legacy_prepare_inspects_unread_observation: a proved negative control.

   Rust correspondence: rholang/src/rust/interpreter/accounting/
   native_runtime/replay_authority.rs (ReplayAuthorityBinding::prepare and
   ReplayAuthorityPublication::publish). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import ObservationReadCoverage CandidateReadCoverage.
Import ListNotations.

Lemma total_nil : forall project, total project [] = 0.
Proof. reflexivity. Qed.

Inductive branch := Retry | Granted | Frontier.

Record comm_row := {
  observation_size : nat;
  saved_size : nat;
  authority_size : nat;
  demand_reads : list nat
}.

Definition prepare_trace (b : branch) (x : comm_row) : list event :=
  match b with
  | Retry =>
      [Reserve (saved_size x); Reserve (observation_size x);
       Read (saved_size x + observation_size x)]
  | Granted =>
      Reserve (authority_size x)
      :: flat_map self_metered (demand_reads x) ++ [Read (authority_size x)]
  | Frontier => [Reserve (authority_size x); Read (authority_size x)]
  end.

Definition legacy_prepare_trace (b : branch) (x : comm_row) : list event :=
  Reserve (observation_size x) :: Reserve (authority_size x)
  :: match b with
     | Retry => [Reserve (saved_size x); Read (saved_size x + observation_size x)]
     | Granted => flat_map self_metered (demand_reads x) ++ [Read (authority_size x)]
     | Frontier => [Read (authority_size x)]
     end.

Theorem prepare_trace_covered : forall b x, covered (prepare_trace b x).
Proof.
  intros [] x; unfold covered.
  - change (covered_from 0
      (map Reserve [saved_size x; observation_size x]
       ++ map Read [saved_size x + observation_size x])).
    apply covered_from_app.
    + apply reserves_covered.
    + rewrite reserve_reads. lia.
    + rewrite reserve_reads, reserve_total. apply reads_covered. simpl. lia.
  - change (covered_from 0
      (map Reserve [authority_size x]
       ++ (flat_map self_metered (demand_reads x) ++ map Read [authority_size x]))).
    apply covered_from_app.
    + apply reserves_covered.
    + rewrite reserve_reads. lia.
    + rewrite reserve_reads, reserve_total. apply covered_from_app.
      * apply flat_self_metered_covered.
      * rewrite flat_self_metered_balanced. lia.
      * rewrite flat_self_metered_balanced, Nat.add_sub. apply reads_covered. simpl. lia.
  - change (covered_from 0 (self_metered (authority_size x))).
    apply self_metered_covered.
Qed.

Theorem legacy_prepare_trace_covered : forall b x, covered (legacy_prepare_trace b x).
Proof.
  intros [] x; unfold covered.
  - change (covered_from 0
      (map Reserve [observation_size x; authority_size x; saved_size x]
       ++ map Read [saved_size x + observation_size x])).
    apply covered_from_app.
    + apply reserves_covered.
    + rewrite reserve_reads. lia.
    + rewrite reserve_reads, reserve_total. apply reads_covered. simpl. lia.
  - change (covered_from 0
      (map Reserve [observation_size x; authority_size x]
       ++ (flat_map self_metered (demand_reads x) ++ map Read [authority_size x]))).
    apply covered_from_app.
    + apply reserves_covered.
    + rewrite reserve_reads. lia.
    + rewrite reserve_reads, reserve_total. apply covered_from_app.
      * apply flat_self_metered_covered.
      * rewrite flat_self_metered_balanced. lia.
      * rewrite flat_self_metered_balanced, Nat.add_sub. apply reads_covered. simpl. lia.
  - change (covered_from 0
      (map Reserve [observation_size x; authority_size x] ++ map Read [authority_size x])).
    apply covered_from_app.
    + apply reserves_covered.
    + rewrite reserve_reads. lia.
    + rewrite reserve_reads, reserve_total. apply reads_covered. simpl. lia.
Qed.

Theorem prepare_reads_equal_legacy_reads : forall b x,
  total read_units (legacy_prepare_trace b x) = total read_units (prepare_trace b x).
Proof.
  intros [] x; unfold legacy_prepare_trace, prepare_trace;
    rewrite ?total_app, ?total_cons, ?total_nil; cbn [read_units]; lia.
Qed.

Definition legacy_excess (b : branch) (x : comm_row) : nat :=
  match b with
  | Retry => authority_size x
  | Granted | Frontier => observation_size x
  end.

Theorem legacy_prepare_excess : forall b x,
  total reserved_units (legacy_prepare_trace b x) =
  total reserved_units (prepare_trace b x) + legacy_excess b x.
Proof.
  intros [] x; unfold legacy_prepare_trace, prepare_trace, legacy_excess;
    rewrite ?total_app, ?total_cons, ?total_nil; cbn [reserved_units]; lia.
Qed.

Definition with_observation (x : comm_row) (size : nat) : comm_row :=
  {| observation_size := size; saved_size := saved_size x;
     authority_size := authority_size x; demand_reads := demand_reads x |}.

Definition with_authority (x : comm_row) (size : nat) : comm_row :=
  {| observation_size := observation_size x; saved_size := saved_size x;
     authority_size := size; demand_reads := demand_reads x |}.

Theorem prepare_charge_independent_of_unread_values : forall x size,
  prepare_trace Granted (with_observation x size) = prepare_trace Granted x /\
  prepare_trace Frontier (with_observation x size) = prepare_trace Frontier x /\
  prepare_trace Retry (with_authority x size) = prepare_trace Retry x.
Proof. intros x size. repeat split. Qed.

(* A granted row with a 4,000-unit observation, a 3-unit authority and a
   2-unit demand: the legacy prepare reserved 4,005 units, D-O5 reserves 5. *)
Example legacy_prepare_inspects_unread_observation :
  total reserved_units (legacy_prepare_trace Granted
    {| observation_size := 4000; saved_size := 0; authority_size := 3; demand_reads := [2] |})
    = 4005 /\
  total reserved_units (prepare_trace Granted
    {| observation_size := 4000; saved_size := 0; authority_size := 3; demand_reads := [2] |})
    = 5.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions prepare_trace_covered.
Print Assumptions legacy_prepare_trace_covered.
Print Assumptions prepare_reads_equal_legacy_reads.
Print Assumptions legacy_prepare_excess.
Print Assumptions prepare_charge_independent_of_unread_values.
Print Assumptions legacy_prepare_inspects_unread_observation.
