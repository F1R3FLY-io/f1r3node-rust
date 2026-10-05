(* D-O4 (epic 8946, Phase D item D-A2; decision record DR-89): the native
   result backing charges only the copies that capture_result makes.

   The model reuses the reservation and read events of
   ObservationReadCoverage. capture_result copies, for every authority
   event, its 32-byte id, its authority and its debit (authority_events). In
   play it also copies the byte-observation rows, which are shared pointers
   whose payload releases were prepaid when the rows were born (the C5 rule):
   one pointer per row. In replay the rows come from the evidence and no row
   is copied.

   Before D-O4 the backing charge walked the whole events map (its tree
   backing and every event, including the shared byte-observation payload of
   each event) and walked every row payload, in play and in replay.

   Results:
   - result_charge_covers_play_copies and
     result_charge_covers_replay_copies: the D-O4 reservations cover every
     copy that follows them (prefix coverage), in play and in replay;
     legacy_result_trace_covered: so did the legacy reservations;
   - result_reads_equal_legacy_reads: the copies are the same;
   - legacy_result_excess: the legacy charge reserved exactly the events
     tree, the event payloads and the deep row walks more, and the row
     pointers less;
   - shared_row_cleanup_releases_no_payload: the D-O4 charge does not depend
     on the row payloads;
   - legacy_result_charge_counts_unreturned_payloads: a proved negative
     control.

   Rust correspondence: rholang/src/rust/interpreter/accounting/
   native_runtime/replay_authority.rs (reserve_native_result_backing) and
   rholang/src/rust/interpreter/interpreter.rs (capture_result). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import ObservationReadCoverage CandidateReadCoverage.
Import ListNotations.

Local Notation sum := (fold_right Nat.add 0).

Lemma result_total_nil : forall project, total project [] = 0.
Proof. reflexivity. Qed.

(* An event: (authority, debit, shared payload) sizes. *)
Definition event_copy (e : nat * nat * nat) : nat := let '(a, d, _) := e in 32 + a + d.
Definition event_payload (e : nat * nat * nat) : nat := let '(_, _, p) := e in p.
Definition event_walk (e : nat * nat * nat) : nat := event_copy e + event_payload e.

Record result_state := {
  events : list (nat * nat * nat);
  event_tree : nat;
  rows : list nat;
  pointer : nat
}.

Definition row_pointers (x : result_state) : nat := pointer x * length (rows x).

(* The copies that capture_result makes; rows only when it copies them
   (play, no evidence). *)
Definition copies (copies_rows : bool) (x : result_state) : list event :=
  map Read (map event_copy (events x))
  ++ (if copies_rows then [Read (row_pointers x)] else []).

Definition result_trace (copies_rows : bool) (x : result_state) : list event :=
  map Reserve (map event_copy (events x))
  ++ ((if copies_rows then [Reserve (row_pointers x)] else []) ++ copies copies_rows x).

Definition legacy_result_trace (copies_rows : bool) (x : result_state) : list event :=
  Reserve (event_tree x + sum (map event_walk (events x)))
  :: Reserve (sum (map (fun payload => pointer x + payload) (rows x)))
  :: copies copies_rows x.

Lemma sum_map_add : forall (A : Type) (f g : A -> nat) values,
  sum (map (fun value => f value + g value) values) = sum (map f values) + sum (map g values).
Proof.
  intros A f g values. induction values as [| value rest IH]; [reflexivity |].
  cbn [map fold_right]. rewrite IH. lia.
Qed.

Lemma walk_sum : forall es,
  sum (map event_walk es) = sum (map event_copy es) + sum (map event_payload es).
Proof. intros es. unfold event_walk. apply sum_map_add. Qed.

Lemma pointer_sum : forall w payloads,
  sum (map (fun payload => w + payload) payloads) = w * length payloads + sum payloads.
Proof.
  intros w payloads. induction payloads as [| payload rest IH]; [cbn; lia |].
  cbn [map fold_right length]. rewrite IH. lia.
Qed.

Lemma reads_reserve_free : forall amounts, total reserved_units (map Read amounts) = 0.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  unfold total in *. cbn [map fold_right reserved_units]. exact IH.
Qed.

Lemma copies_reads : forall c x,
  total read_units (copies c x) =
  sum (map event_copy (events x)) + (if c then row_pointers x else 0).
Proof.
  intros [] x; unfold copies; rewrite total_app, read_total.
  - rewrite total_cons, result_total_nil. cbn [read_units]. lia.
  - rewrite result_total_nil. lia.
Qed.

Lemma copies_reserves : forall c x, total reserved_units (copies c x) = 0.
Proof.
  intros [] x; unfold copies; rewrite total_app, reads_reserve_free.
  - rewrite total_cons, result_total_nil. reflexivity.
  - rewrite result_total_nil. reflexivity.
Qed.

Theorem result_charge_covers_copies : forall c x, covered (result_trace c x).
Proof.
  intros c x. unfold covered, result_trace.
  apply covered_from_app.
  - apply reserves_covered.
  - rewrite reserve_reads. lia.
  - rewrite reserve_reads, reserve_total, Nat.sub_0_r, Nat.add_0_l.
    destruct c.
    + change ([Reserve (row_pointers x)] ++ copies true x)
        with (map Reserve [row_pointers x] ++ copies true x).
      apply covered_from_app.
      * apply reserves_covered.
      * rewrite reserve_reads. lia.
      * rewrite reserve_reads, reserve_total, Nat.sub_0_r.
        intros prefix suffix split.
        assert (bound : total read_units prefix <= total read_units (copies true x)).
        { rewrite split, total_app. lia. }
        rewrite copies_reads in bound. cbn [fold_right]. lia.
    + change ([] ++ copies false x) with (copies false x).
      intros prefix suffix split.
      assert (bound : total read_units prefix <= total read_units (copies false x)).
      { rewrite split, total_app. lia. }
      rewrite copies_reads in bound. lia.
Qed.

Corollary result_charge_covers_play_copies : forall x, covered (result_trace true x).
Proof. intros x. apply result_charge_covers_copies. Qed.

Corollary result_charge_covers_replay_copies : forall x, covered (result_trace false x).
Proof. intros x. apply result_charge_covers_copies. Qed.

Theorem legacy_result_trace_covered : forall c x, covered (legacy_result_trace c x).
Proof.
  intros c x. unfold covered, legacy_result_trace.
  change (Reserve (event_tree x + sum (map event_walk (events x)))
          :: Reserve (sum (map (fun payload => pointer x + payload) (rows x)))
          :: copies c x)
    with (map Reserve [event_tree x + sum (map event_walk (events x));
                       sum (map (fun payload => pointer x + payload) (rows x))]
          ++ copies c x).
  apply covered_from_app.
  - apply reserves_covered.
  - rewrite reserve_reads. lia.
  - rewrite reserve_reads, reserve_total.
    intros prefix suffix split.
    assert (bound : total read_units prefix <= total read_units (copies c x)).
    { rewrite split, total_app. lia. }
    rewrite copies_reads in bound. unfold row_pointers in bound.
    cbn [fold_right]. rewrite walk_sum, pointer_sum. destruct c; lia.
Qed.

Theorem result_reads_equal_legacy_reads : forall c x,
  total read_units (legacy_result_trace c x) = total read_units (result_trace c x).
Proof.
  intros c x. unfold legacy_result_trace, result_trace.
  rewrite !total_cons, !total_app, reserve_reads. cbn [read_units].
  destruct c.
  - rewrite total_cons, result_total_nil. cbn [read_units]. lia.
  - rewrite result_total_nil. lia.
Qed.

Theorem legacy_result_excess : forall c x,
  total reserved_units (legacy_result_trace c x) + (if c then row_pointers x else 0) =
  total reserved_units (result_trace c x) + event_tree x + sum (map event_payload (events x))
  + sum (map (fun payload => pointer x + payload) (rows x)).
Proof.
  intros c x. unfold legacy_result_trace, result_trace.
  rewrite !total_cons, !total_app, reserve_total, copies_reserves, walk_sum.
  cbn [reserved_units].
  destruct c.
  - rewrite total_cons, result_total_nil. cbn [reserved_units]. lia.
  - rewrite result_total_nil. lia.
Qed.

Definition with_rows (x : result_state) (payloads : list nat) : result_state :=
  {| events := events x; event_tree := event_tree x; rows := payloads; pointer := pointer x |}.

Theorem shared_row_cleanup_releases_no_payload : forall c x payloads,
  length payloads = length (rows x) ->
  result_trace c (with_rows x payloads) = result_trace c x.
Proof.
  intros c x payloads same. unfold result_trace, copies, row_pointers, with_rows.
  cbn [events rows pointer]. rewrite same. reflexivity.
Qed.

(* One event with a 2,000-unit shared payload and one row with a 500-unit
   payload, replayed with evidence: the legacy backing reserved 2,648 units
   (tree 100 + (32 + 5 + 3 + 2000) + (8 + 500)); D-O4 reserves 40
   (32 + 5 + 3). *)
Example legacy_result_charge_counts_unreturned_payloads :
  total reserved_units (legacy_result_trace false
    {| events := [(5, 3, 2000)]; event_tree := 100; rows := [500]; pointer := 8 |}) = 2648 /\
  total reserved_units (result_trace false
    {| events := [(5, 3, 2000)]; event_tree := 100; rows := [500]; pointer := 8 |}) = 40.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions result_charge_covers_play_copies.
Print Assumptions result_charge_covers_replay_copies.
Print Assumptions legacy_result_trace_covered.
Print Assumptions result_reads_equal_legacy_reads.
Print Assumptions legacy_result_excess.
Print Assumptions shared_row_cleanup_releases_no_payload.
Print Assumptions legacy_result_charge_counts_unreturned_payloads.
