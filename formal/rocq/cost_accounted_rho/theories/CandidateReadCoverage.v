(* D-M1 and D-M6 (epic 8946, Phase D; decision record DR-88): native
   candidate selection charges only what the matcher reads.

   The model reuses the reservation and read events of
   ObservationReadCoverage (C12): Reserve u adds u units to the host-work
   budget, Read u is work that no other step meters, and a self-metered step
   is the balanced pair Reserve u; Read u. The metering rule is prefix
   coverage: every prefix of a run reads at most the units that the same
   prefix has reserved.

   A match attempt calls the metered matcher (Match::get_metered), which
   reserves each read of the pattern and the datum before it performs it, so
   its work is a list of self-metered steps. Before D-M1 the caller also
   inspected the whole pattern and the whole datum first, and those
   inspections read nothing.

   A commit check calls Match::check_commit_metered, which reserves each read
   of the guard and of the matched bindings before it performs it. Before
   D-M1 the caller also inspected the whole continuation first, which read
   nothing. Before D-M6 the caller also copied every matched datum into an
   owned vector only to pass a slice; D-M6 passes a vector of references.

   Results:
   - attempt_trace_covered, legacy_attempt_trace_covered,
     commit_trace_covered, legacy_commit_trace_covered: every run satisfies
     prefix coverage.
   - attempt_reads_equal_legacy_reads: the D-M1 attempt performs the same
     reads as the legacy attempt; legacy_attempt_excess: the legacy attempt
     reserves exactly one pattern inspection and one datum inspection more.
   - legacy_commit_excess: the legacy commit check reserves exactly the
     continuation inspection, the owned buffer and the datum copies more, and
     the reference buffer less; legacy_commit_reads: the legacy commit check
     also performed the owned buffer and the copies as real work, and the
     continuation inspection read nothing.
   - attempt_charge_independent_of_unread_tails,
     commit_charge_independent_of_body,
     commit_refs_charge_independent_of_matched_sizes: the new charges do not
     depend on the unread pattern and datum, on the continuation, or on the
     sizes of the matched data (only on their number).
   - Negative controls legacy_commit_charge_depends_on_unread_body,
     legacy_attempt_charge_depends_on_unread_tails and
     legacy_commit_copy_excess compute concrete legacy excesses.

   Rust correspondence: rspace++/src/rspace/replay_rspace/native_candidate/
   metered.rs (metered_match_data), rspace++/src/rspace/replay_rspace/
   native_session/installation.rs (install), rspace++/src/rspace/match.rs
   (the self-metering contract), rholang/src/rust/interpreter/matcher/
   match.rs (Matcher::check_commit_metered). The extracted tests are
   match_attempt_charge_excludes_unread_pattern_and_datum,
   commit_check_charge_is_independent_of_continuation_body and
   legacy_selection_charge_grew_with_unread_values (rspace++ metered
   tests), and commit_guard_by_reference_matches_owned_guard,
   commit_check_charge_is_independent_of_continuation_body and
   legacy_commit_charge_grew_with_unread_body (rholang matcher tests). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import ObservationReadCoverage.
Import ListNotations.

Lemma total_cons : forall project e rest,
  total project (e :: rest) = project e + total project rest.
Proof. reflexivity. Qed.

Lemma self_metered_reserved : forall units,
  total reserved_units (self_metered units) = units.
Proof. intros units. unfold total, self_metered. simpl. lia. Qed.

Lemma self_metered_reads : forall units,
  total read_units (self_metered units) = units.
Proof. intros units. unfold total, self_metered. simpl. lia. Qed.

Lemma flat_self_metered_reserved : forall amounts,
  total reserved_units (flat_map self_metered amounts) = fold_right Nat.add 0 amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  change (flat_map self_metered (amount :: rest))
    with (self_metered amount ++ flat_map self_metered rest).
  rewrite total_app, self_metered_reserved, IH. reflexivity.
Qed.

Lemma flat_self_metered_reads : forall amounts,
  total read_units (flat_map self_metered amounts) = fold_right Nat.add 0 amounts.
Proof.
  intros amounts. rewrite flat_self_metered_balanced. apply flat_self_metered_reserved.
Qed.

(* A match attempt: the pattern and datum sizes are what the legacy caller
   inspected; matcher_reads are the self-metered reads of get_metered. *)
Record attempt := {
  pattern_size : nat;
  datum_size : nat;
  matcher_reads : list nat
}.

Definition attempt_trace (x : attempt) : list event :=
  flat_map self_metered (matcher_reads x).

Definition legacy_attempt_trace (x : attempt) : list event :=
  Reserve (pattern_size x) :: Reserve (datum_size x) :: attempt_trace x.

Theorem attempt_trace_covered : forall x, covered (attempt_trace x).
Proof. intros x. unfold covered, attempt_trace. apply flat_self_metered_covered. Qed.

Theorem legacy_attempt_trace_covered : forall x, covered (legacy_attempt_trace x).
Proof.
  intros x. unfold covered, legacy_attempt_trace.
  change (Reserve (pattern_size x) :: Reserve (datum_size x) :: attempt_trace x)
    with (map Reserve [pattern_size x; datum_size x] ++ attempt_trace x).
  apply covered_from_app.
  - apply reserves_covered.
  - rewrite reserve_reads. lia.
  - rewrite reserve_reads, reserve_total.
    intros prefix suffix split.
    pose proof (attempt_trace_covered x prefix suffix split). lia.
Qed.

Theorem attempt_reads_equal_legacy_reads : forall x,
  total read_units (legacy_attempt_trace x) = total read_units (attempt_trace x).
Proof. intros x. unfold legacy_attempt_trace, total. simpl. reflexivity. Qed.

Theorem legacy_attempt_excess : forall x,
  total reserved_units (legacy_attempt_trace x) =
  total reserved_units (attempt_trace x) + pattern_size x + datum_size x.
Proof. intros x. unfold legacy_attempt_trace, total. simpl. lia. Qed.

Definition with_unread_tails (x : attempt) (pattern datum : nat) : attempt :=
  {| pattern_size := pattern; datum_size := datum; matcher_reads := matcher_reads x |}.

Theorem attempt_charge_independent_of_unread_tails : forall x pattern datum,
  attempt_trace (with_unread_tails x pattern datum) = attempt_trace x.
Proof. reflexivity. Qed.

(* A commit check: continuation_size is the legacy inspection of the whole
   continuation; guard_reads are the self-metered reads of
   check_commit_metered; matched_sizes are the copy work of each matched
   datum in the legacy owned vector; reference_slot and owned_slot are the
   per-element bytes of the reference vector (D-M6) and of the legacy owned
   vector. *)
Record commit := {
  continuation_size : nat;
  guard_reads : list nat;
  matched_sizes : list nat;
  reference_slot : nat;
  owned_slot : nat
}.

Definition commit_trace (x : commit) : list event :=
  self_metered (reference_slot x * length (matched_sizes x))
  ++ flat_map self_metered (guard_reads x).

Definition legacy_commit_trace (x : commit) : list event :=
  self_metered (owned_slot x * length (matched_sizes x))
  ++ flat_map self_metered (matched_sizes x)
  ++ Reserve (continuation_size x) :: flat_map self_metered (guard_reads x).

Theorem commit_trace_covered : forall x, covered (commit_trace x).
Proof.
  intros x. unfold covered, commit_trace.
  apply covered_from_app.
  - apply self_metered_covered.
  - rewrite self_metered_balanced. lia.
  - rewrite self_metered_balanced, Nat.add_sub. apply flat_self_metered_covered.
Qed.

Theorem legacy_commit_trace_covered : forall x, covered (legacy_commit_trace x).
Proof.
  intros x. unfold covered, legacy_commit_trace.
  apply covered_from_app.
  - apply self_metered_covered.
  - rewrite self_metered_balanced. lia.
  - rewrite self_metered_balanced, Nat.add_sub.
    apply covered_from_app.
    + apply flat_self_metered_covered.
    + rewrite flat_self_metered_balanced. lia.
    + rewrite flat_self_metered_balanced, Nat.add_sub.
      change (Reserve (continuation_size x) :: flat_map self_metered (guard_reads x))
        with (map Reserve [continuation_size x] ++ flat_map self_metered (guard_reads x)).
      apply covered_from_app.
      * apply reserves_covered.
      * rewrite reserve_reads. lia.
      * rewrite reserve_reads, reserve_total.
        intros prefix suffix split.
        pose proof (flat_self_metered_covered 0 (guard_reads x) prefix suffix split). lia.
Qed.

Theorem legacy_commit_excess : forall x,
  total reserved_units (legacy_commit_trace x) + reference_slot x * length (matched_sizes x) =
  total reserved_units (commit_trace x) + owned_slot x * length (matched_sizes x)
  + fold_right Nat.add 0 (matched_sizes x) + continuation_size x.
Proof.
  intros x. unfold legacy_commit_trace, commit_trace.
  rewrite !total_app, total_cons, !self_metered_reserved, !flat_self_metered_reserved.
  cbn [reserved_units]. lia.
Qed.

Theorem legacy_commit_reads : forall x,
  total read_units (legacy_commit_trace x) + reference_slot x * length (matched_sizes x) =
  total read_units (commit_trace x) + owned_slot x * length (matched_sizes x)
  + fold_right Nat.add 0 (matched_sizes x).
Proof.
  intros x. unfold legacy_commit_trace, commit_trace.
  rewrite !total_app, total_cons, !self_metered_reads, !flat_self_metered_reads.
  cbn [read_units]. lia.
Qed.

Definition with_continuation (x : commit) (size : nat) : commit :=
  {| continuation_size := size; guard_reads := guard_reads x;
     matched_sizes := matched_sizes x; reference_slot := reference_slot x;
     owned_slot := owned_slot x |}.

Theorem commit_charge_independent_of_body : forall x size,
  commit_trace (with_continuation x size) = commit_trace x.
Proof. reflexivity. Qed.

Definition with_matched (x : commit) (sizes : list nat) : commit :=
  {| continuation_size := continuation_size x; guard_reads := guard_reads x;
     matched_sizes := sizes; reference_slot := reference_slot x;
     owned_slot := owned_slot x |}.

Theorem commit_refs_charge_independent_of_matched_sizes : forall x sizes,
  length sizes = length (matched_sizes x) ->
  commit_trace (with_matched x sizes) = commit_trace x.
Proof.
  intros x sizes same. unfold commit_trace, with_matched.
  cbn [matched_sizes guard_reads reference_slot]. rewrite same. reflexivity.
Qed.

Example legacy_commit_charge_depends_on_unread_body :
  total read_units (legacy_commit_trace
    (with_continuation {| continuation_size := 1; guard_reads := [2]; matched_sizes := [3];
                          reference_slot := 8; owned_slot := 24 |} 1000)) =
  total read_units (legacy_commit_trace
    {| continuation_size := 1; guard_reads := [2]; matched_sizes := [3];
       reference_slot := 8; owned_slot := 24 |}) /\
  Nat.eqb
    (total reserved_units (legacy_commit_trace
      (with_continuation {| continuation_size := 1; guard_reads := [2]; matched_sizes := [3];
                            reference_slot := 8; owned_slot := 24 |} 1000)))
    (total reserved_units (legacy_commit_trace
      {| continuation_size := 1; guard_reads := [2]; matched_sizes := [3];
         reference_slot := 8; owned_slot := 24 |})) = false.
Proof. split; vm_compute; reflexivity. Qed.

Example legacy_attempt_charge_depends_on_unread_tails :
  total read_units (legacy_attempt_trace
    (with_unread_tails {| pattern_size := 1; datum_size := 1; matcher_reads := [2] |} 4096 4096)) =
  total read_units (legacy_attempt_trace
    {| pattern_size := 1; datum_size := 1; matcher_reads := [2] |}) /\
  Nat.eqb
    (total reserved_units (legacy_attempt_trace
      (with_unread_tails {| pattern_size := 1; datum_size := 1; matcher_reads := [2] |} 4096 4096)))
    (total reserved_units (legacy_attempt_trace
      {| pattern_size := 1; datum_size := 1; matcher_reads := [2] |})) = false.
Proof. split; vm_compute; reflexivity. Qed.

(* Two matched data of copy work 100 and 200, a guard read of 2 units and a
   continuation inspection of 5 units: the legacy check reserved 355 units
   (24 * 2 + 300 + 5 + 2), the D-M1/D-M6 check reserves 18 (8 * 2 + 2). *)
Example legacy_commit_copy_excess :
  total reserved_units (legacy_commit_trace
    {| continuation_size := 5; guard_reads := [2]; matched_sizes := [100; 200];
       reference_slot := 8; owned_slot := 24 |}) = 355 /\
  total reserved_units (commit_trace
    {| continuation_size := 5; guard_reads := [2]; matched_sizes := [100; 200];
       reference_slot := 8; owned_slot := 24 |}) = 18.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions attempt_trace_covered.
Print Assumptions legacy_attempt_trace_covered.
Print Assumptions attempt_reads_equal_legacy_reads.
Print Assumptions legacy_attempt_excess.
Print Assumptions attempt_charge_independent_of_unread_tails.
Print Assumptions commit_trace_covered.
Print Assumptions legacy_commit_trace_covered.
Print Assumptions legacy_commit_excess.
Print Assumptions legacy_commit_reads.
Print Assumptions commit_charge_independent_of_body.
Print Assumptions commit_refs_charge_independent_of_matched_sizes.
Print Assumptions legacy_commit_charge_depends_on_unread_body.
Print Assumptions legacy_attempt_charge_depends_on_unread_tails.
Print Assumptions legacy_commit_copy_excess.
