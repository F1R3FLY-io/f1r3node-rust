(* ═══════════════════════════════════════════════════════════════════════════
   AgentReadCoverage.v — a space agent reserves its reads before it makes them
   (decision 42, P4)

   Decision 42 aligns the metering of the P4 queue agent with DR-88 of
   feature/cost-accounted-rho: the agent reserves every read and every copy
   that it makes, before it makes it, on the host-work budget of the deploy.
   This module restates the metering rule of that branch and proves that the
   collection operations of the agent satisfy it.

   The rule (restated from the branch, ObservationReadCoverage.v of DR-76 and
   CandidateReadCoverage.v of DR-88). A metered computation is a list of
   events:
   - Reserve u adds u units to the host-work budget;
   - Read u is work of u units that no other step meters.
   A self-metered step reserves its work and then performs it, so it is the
   balanced pair Reserve u; Read u. The rule is prefix coverage: every prefix
   of a run reads at most the units that the same prefix has reserved. The
   three host-work dimensions of a reservation (operations, scanned bytes,
   backing bytes) each satisfy the rule on their own, so the model counts one
   dimension; each theorem holds for each dimension.

   Sources
   - Design: .reified-rspaces-work/p4/decisions-41-43-design.md, section 42
     (42.1 the policy of the branch, 42.3 every site, 42.6 this model).
     Decision 42 of docs/rspace/reified-rspaces/decision-register.md.
   - Code: the commit of decision 42 in this repository, with the re-entry
     of decision 43 and the re-store of decision 45 (Rule E). The line
     numbers below are the line numbers of the commit of decision 45.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   step                     reified/costs.rs:97-102, 106-111, 123-126
                            (ScanMeter::inspect, reserve_copy, copy:
                            reserve, then work)
   scan                     spaces/generic.rs:586-602, 606-618, 622-634
                            (scan_head, head_waiter, head_datum: the
                            buffer, one envelope decode for each entry, the
                            head copy) and
                            spaces/disciplines/queue.rs:39-57, 86-121
                            (head_metered: one comparison for each entry
                            after the first)
   round_staged             spaces/generic.rs:696-759, 767-838 (offer and
                            receive: the scan, the payload decode of a
                            receive, the metered match, the prepaid unwrap,
                            the staged fire). For a peek receive of a
                            non-persistent record, the staging of the round
                            also holds the re-store of decision 45: the
                            reserved copy of the record and the steps of
                            Plan::restore (spaces/generic.rs:347-380: the
                            channel copy, Introduction::for_record and
                            register_introduction), each self-metered
   round_after_commit       spaces/generic.rs:911-946 (outcome: the moves of
                            into_continuation, prepaid by
                            spaces/record.rs:721-730)
   operation_trace          spaces/generic.rs:980-1048 (prepare_produce and
                            prepare_consume: the channels, the config, the
                            matcher handle, the introduction, the bump, the
                            record, the rounds and the prepaid outcome
                            buffer). The checks of a re-entry (decision 43)
                            are self-metered steps of prepare: the two
                            comparisons of operation_ord
                            (spaces/generic.rs:856-868), and the scan of
                            head_ord and at_head (spaces/generic.rs:638-655,
                            663-688: the buffer and the envelope decodes of
                            scan_head, with no copy) with the comparison of
                            the two orders and QueueDiscipline::precedes
                            (spaces/disciplines/queue.rs:63-72)
   p4_scan                  the P4 commit 294c95632: head_waiter and
                            head_datum decoded each envelope and copied the
                            head without a reservation

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - The metered matcher, a metered encode and a reserved copy each reserve
     before they read, so the model writes their work as self-metered steps.
     The units of each step are its reservation.
   - An entry of a side of a collection has the units of the decode of its
     envelope and the units of a copy of the whole entry. A scan decodes the
     envelope of each entry and copies only the head.
   - The commit applies the staged operations. It is host work of the
     substrate, which the branch does not meter on its play path, so it is
     not an event. The outcome runs after the commit: it moves each fired
     continuation out of its receipt and fills the prepaid buffer of fires.

   ─────────────────────────────────────────────────────────────────────────
   Results (8 headlines, axiom-free)
   ─────────────────────────────────────────────────────────────────────────
   - operation_trace_covered: every collection operation satisfies prefix
     coverage, its reads after the commit included.
   - reservations_precede_commit: the trace after the commit reserves
     nothing.
   - cut_rejects_before_commit: a budget below the reservations of an
     operation is exceeded by a reservation of the staged part, so the
     operation fails before the commit and has no effect.
   - scan_charge_permutation_invariant and
     operation_charge_permutation_invariant: the charges do not depend on the
     list order of the staged view.
   - scan_charge_independent_of_unread_payloads: the payloads of the entries
     that a scan does not copy do not change its charge.
   - Negative controls: p4_scan_covered_only_if_free (the P4 scan, which
     reserved nothing for its own decodes and copies, satisfies the rule only
     when that work is zero) and unprepaid_trace_uncovered (without the
     prepaid reservations, the reads of the outcome after the commit break
     the rule).

   Rust correspondence of the extracted tests: rholang/tests/
   reified_p4_props.rs (prop_agent_reservation_cut_rejects_without_effect,
   prop_queue_plan_permutation_invariant,
   prop_agent_charge_independent_of_unread_payloads,
   prop_agent_reserves_its_own_work_beyond_the_matcher), and the witness
   test space_counter_bump_cut_budget_rejects_without_effect
   (spaces/counter.rs).
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── The metering rule of DR-88 ─────────────────────────────────────────── *)

Inductive event := Reserve (units : nat) | Read (units : nat).

Definition reserved_units (e : event) : nat :=
  match e with Reserve units => units | Read _ => 0 end.

Definition read_units (e : event) : nat :=
  match e with Read units => units | Reserve _ => 0 end.

Definition total (project : event -> nat) (trace : list event) : nat :=
  fold_right Nat.add 0 (map project trace).

Lemma total_nil : forall project, total project [] = 0.
Proof. reflexivity. Qed.

Lemma total_cons : forall project e rest,
  total project (e :: rest) = project e + total project rest.
Proof. reflexivity. Qed.

Lemma total_app : forall project left right,
  total project (left ++ right) = total project left + total project right.
Proof.
  intros project left right. unfold total.
  rewrite map_app, fold_right_app.
  induction left as [| e rest IH]; simpl; [reflexivity |].
  unfold total in IH. rewrite IH. lia.
Qed.

(* Every prefix reads at most [slack] units more than it reserves. *)
Definition covered_from (slack : nat) (trace : list event) : Prop :=
  forall prefix suffix, trace = prefix ++ suffix ->
    total read_units prefix <= slack + total reserved_units prefix.

Definition covered (trace : list event) : Prop := covered_from 0 trace.

Lemma covered_from_app : forall slack left right,
  covered_from slack left ->
  total read_units left <= slack + total reserved_units left ->
  covered_from (slack + total reserved_units left - total read_units left) right ->
  covered_from slack (left ++ right).
Proof.
  intros slack left right covered_left fits covered_right prefix suffix split.
  apply app_eq_app in split as [middle [[left_prefix right_suffix] | [prefix_left right_middle]]].
  - (* the prefix ends inside [left] *)
    exact (covered_left prefix middle left_prefix).
  - (* the prefix covers all of [left] *)
    subst prefix.
    pose proof (covered_right middle suffix right_middle) as inner.
    rewrite !total_app. lia.
Qed.

Lemma covered_from_weaken : forall slack more trace,
  covered_from slack trace -> slack <= more -> covered_from more trace.
Proof.
  intros slack more trace covered_trace less prefix suffix split.
  pose proof (covered_trace prefix suffix split). lia.
Qed.

Lemma covered_from_nil : forall slack, covered_from slack [].
Proof.
  intros slack prefix suffix split.
  destruct prefix; [rewrite total_nil; lia | discriminate].
Qed.

(* A self-metered step: reserve the work, then perform it. *)
Definition step (units : nat) : list event := [Reserve units; Read units].

Lemma step_covered : forall slack units, covered_from slack (step units).
Proof.
  intros slack units prefix suffix split.
  unfold step in split.
  destruct prefix as [| a [| b [| c rest]]]; simpl in split.
  - rewrite total_nil. lia.
  - injection split as ha _. subst a. unfold total. simpl. lia.
  - injection split as ha hb _. subst a b. unfold total. simpl. lia.
  - discriminate.
Qed.

Definition sum (amounts : list nat) : nat := fold_right Nat.add 0 amounts.

Lemma sum_nil : sum [] = 0.
Proof. reflexivity. Qed.

Lemma sum_cons : forall amount rest, sum (amount :: rest) = amount + sum rest.
Proof. reflexivity. Qed.

Arguments sum : simpl never.

Lemma steps_reserved : forall amounts,
  total reserved_units (flat_map step amounts) = sum amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  change (flat_map step (amount :: rest)) with (step amount ++ flat_map step rest).
  rewrite total_app, IH, sum_cons. unfold total, step. simpl. lia.
Qed.

Lemma steps_read : forall amounts,
  total read_units (flat_map step amounts) = sum amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  change (flat_map step (amount :: rest)) with (step amount ++ flat_map step rest).
  rewrite total_app, IH, sum_cons. unfold total, step. simpl. lia.
Qed.

Lemma steps_covered : forall slack amounts, covered_from slack (flat_map step amounts).
Proof.
  intros slack amounts. induction amounts as [| amount rest IH].
  - apply covered_from_nil.
  - change (covered_from slack (step amount ++ flat_map step rest)).
    apply covered_from_app.
    + apply step_covered.
    + unfold total, step. simpl. lia.
    + replace (slack + total reserved_units (step amount) - total read_units (step amount))
        with slack by (unfold total, step; simpl; lia).
      exact IH.
Qed.

(* A balanced segment: covered from any slack, and it leaves the slack as it
   is. *)
Definition balanced (trace : list event) : Prop :=
  (forall slack, covered_from slack trace) /\
  total read_units trace = total reserved_units trace.

Lemma steps_balanced : forall amounts, balanced (flat_map step amounts).
Proof.
  intros amounts. split; [intros slack; apply steps_covered |].
  rewrite steps_read, steps_reserved. reflexivity.
Qed.

Lemma balanced_app : forall left right,
  balanced left -> balanced right -> balanced (left ++ right).
Proof.
  intros left right [covered_left equal_left] [covered_right equal_right].
  split.
  - intros slack. apply covered_from_app; [apply covered_left | lia |].
    replace (slack + total reserved_units left - total read_units left) with slack by lia.
    apply covered_right.
  - rewrite !total_app. lia.
Qed.

(* The first reservation that exceeds a budget. *)
Lemma first_crossing : forall trace budget,
  budget < total reserved_units trace ->
  exists prefix units suffix,
    trace = prefix ++ Reserve units :: suffix /\
    total reserved_units prefix <= budget /\
    budget < total reserved_units prefix + units.
Proof.
  induction trace as [| e rest IH]; intros budget exceeds.
  - rewrite total_nil in exceeds. lia.
  - rewrite total_cons in exceeds.
    destruct (Nat.lt_ge_cases budget (reserved_units e)) as [crosses | fits].
    + destruct e as [units | units]; simpl in crosses; [| lia].
      exists [], units, rest. rewrite total_nil. repeat split; lia.
    + destruct (IH (budget - reserved_units e)) as [prefix [units [suffix [shape [below above]]]]];
        [lia |].
      exists (e :: prefix), units, suffix. rewrite total_cons, shape.
      repeat split; lia.
Qed.

(* ── The agent ──────────────────────────────────────────────────────────── *)

(* An entry of one side of a collection in the staged view: the units of the
   decode of its envelope, and the units of a copy of the whole entry. *)
Record entry := { envelope : nat; payload : nat }.

Section Agent.

(* The units of a buffer of n slots, and of one comparison of two orders. *)
Variable buffer : nat -> nat.
Variable compare : nat.

(* head_waiter or head_datum with head_metered: the buffer of the ordered
   view, the decode of each envelope, one comparison for each entry after the
   first, and the copy of the head. *)
Definition scan (side : list entry) (head : entry) : list event :=
  step (buffer (length side))
  ++ flat_map step (map envelope side)
  ++ flat_map step (map (fun _ => compare) (tl side))
  ++ step (payload head).

(* One round of the fire loop of an offer or of a receive. *)
Record round := {
  side : list entry;    (* the side that the round scans *)
  head : entry;         (* the head that the round meets *)
  decoded : list nat;   (* the decodes before the match: the payload of the
                           head record for a receive, none for an offer *)
  matcher : list nat;   (* the self-metered reads of get_metered and
                           check_commit_metered *)
  fires : bool;         (* the round stages a fire *)
  unwrap : nat;         (* the decode of the fired continuation, which the
                           round prepays *)
  staging : list nat    (* the copies and buffers of the staged operation *)
}.

Definition prepaid (r : round) : nat := if fires r then unwrap r else 0.

Definition round_staged (r : round) : list event :=
  scan (side r) (head r)
  ++ flat_map step (decoded r)
  ++ flat_map step (matcher r)
  ++ (if fires r then [Reserve (unwrap r)] else [])
  ++ flat_map step (staging r).

Definition round_after_commit (r : round) : list event :=
  if fires r then [Read (unwrap r)] else [].

Record operation := {
  setup : list nat;     (* the channels, the config read and decode, the
                           matcher handle and the introduction *)
  bump : list nat;      (* the counter bump: the copy and the decode of the
                           counter record, the matcher reads of its take, the
                           templates, the random bytes and the copies *)
  prepare : list nat;   (* the record of the operation, its identity, and
                           the checks of a re-entry (decision 43) *)
  rounds : list round;  (* the fire loop *)
  outcome : nat         (* the buffer of the fires of the outcome, which the
                           operation prepays *)
}.

Definition staged_trace (x : operation) : list event :=
  flat_map step (setup x) ++ flat_map step (bump x) ++ flat_map step (prepare x)
  ++ flat_map round_staged (rounds x) ++ [Reserve (outcome x)].

Definition after_commit_trace (x : operation) : list event :=
  Read (outcome x) :: flat_map round_after_commit (rounds x).

Definition operation_trace (x : operation) : list event :=
  staged_trace x ++ after_commit_trace x.

Lemma scan_balanced : forall side head, balanced (scan side head).
Proof.
  intros side head. unfold scan.
  repeat apply balanced_app; apply steps_balanced || apply (steps_balanced [_]).
Qed.

Lemma reserve_one_read : forall units, total read_units [Reserve units] = 0.
Proof. reflexivity. Qed.

Lemma reserve_one_reserved : forall units, total reserved_units [Reserve units] = units.
Proof. intros units. unfold total. simpl. lia. Qed.

Lemma reserve_covered : forall slack units, covered_from slack [Reserve units].
Proof.
  intros slack units prefix suffix split.
  destruct prefix as [| a [| b rest]]; simpl in split.
  - rewrite total_nil. lia.
  - injection split as ha _. subst a. unfold total. simpl. lia.
  - discriminate.
Qed.

(* A staged round is covered, and it leaves its prepaid units as slack. *)
Lemma round_staged_covered : forall slack r, covered_from slack (round_staged r).
Proof.
  intros slack r. unfold round_staged.
  pose proof (scan_balanced (side r) (head r)) as [scan_covered scan_equal].
  pose proof (steps_balanced (decoded r ++ matcher r)) as [pre_covered pre_equal].
  rewrite flat_map_app in pre_covered, pre_equal.
  apply covered_from_app; [apply scan_covered | lia |].
  replace (slack + total reserved_units (scan (side r) (head r))
           - total read_units (scan (side r) (head r))) with slack by lia.
  rewrite app_assoc.
  apply covered_from_app; [apply pre_covered | lia |].
  apply covered_from_app.
  - destruct (fires r); [apply reserve_covered | apply covered_from_nil].
  - destruct (fires r); unfold total; simpl; lia.
  - apply steps_covered.
Qed.

Lemma round_staged_totals : forall r,
  total read_units (round_staged r) + prepaid r = total reserved_units (round_staged r).
Proof.
  intros r. unfold round_staged, prepaid.
  pose proof (scan_balanced (side r) (head r)) as [_ scan_equal].
  rewrite !total_app, !steps_read, !steps_reserved.
  destruct (fires r); unfold total at 2 4; simpl; lia.
Qed.

Lemma rounds_staged_covered : forall slack rs,
  covered_from slack (flat_map round_staged rs).
Proof.
  intros slack rs. revert slack.
  induction rs as [| r rest IH]; intros slack; [apply covered_from_nil |].
  change (covered_from slack (round_staged r ++ flat_map round_staged rest)).
  pose proof (round_staged_totals r) as totals.
  apply covered_from_app; [apply round_staged_covered | lia | apply IH].
Qed.

Lemma rounds_staged_totals : forall rs,
  total read_units (flat_map round_staged rs) + sum (map prepaid rs) =
  total reserved_units (flat_map round_staged rs).
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map round_staged (r :: rest)) with (round_staged r ++ flat_map round_staged rest).
  pose proof (round_staged_totals r).
  cbn [map]. rewrite !total_app, sum_cons. lia.
Qed.

Lemma after_rounds_read : forall rs,
  total read_units (flat_map round_after_commit rs) = sum (map prepaid rs).
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map round_after_commit (r :: rest))
    with (round_after_commit r ++ flat_map round_after_commit rest).
  cbn [map]. rewrite total_app, IH, sum_cons. unfold round_after_commit, prepaid.
  destruct (fires r); unfold total; simpl; lia.
Qed.

Lemma after_rounds_reserved : forall rs,
  total reserved_units (flat_map round_after_commit rs) = 0.
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map round_after_commit (r :: rest))
    with (round_after_commit r ++ flat_map round_after_commit rest).
  rewrite total_app, IH. unfold round_after_commit.
  destruct (fires r); unfold total; simpl; lia.
Qed.

(* A list of reads is covered from a slack at least its units. *)
Lemma reads_only_covered : forall slack trace,
  total reserved_units trace = 0 ->
  total read_units trace <= slack ->
  covered_from slack trace.
Proof.
  intros slack trace no_reserve bound prefix suffix split.
  subst trace. rewrite total_app in bound. lia.
Qed.

Lemma staged_totals : forall x,
  total read_units (staged_trace x) + outcome x + sum (map prepaid (rounds x)) =
  total reserved_units (staged_trace x).
Proof.
  intros x. unfold staged_trace.
  pose proof (rounds_staged_totals (rounds x)).
  rewrite !total_app, !steps_read, !steps_reserved, reserve_one_read, reserve_one_reserved.
  lia.
Qed.

Lemma staged_covered : forall slack x, covered_from slack (staged_trace x).
Proof.
  intros slack x.
  assert (shape : staged_trace x =
    flat_map step (setup x ++ bump x ++ prepare x) ++
    (flat_map round_staged (rounds x) ++ [Reserve (outcome x)])).
  { unfold staged_trace. rewrite !flat_map_app, <- !app_assoc. reflexivity. }
  rewrite shape.
  pose proof (steps_balanced (setup x ++ bump x ++ prepare x)) as [steps_cov steps_equal].
  pose proof (rounds_staged_totals (rounds x)) as round_totals.
  apply covered_from_app; [apply steps_cov | lia |].
  apply covered_from_app; [apply rounds_staged_covered | lia | apply reserve_covered].
Qed.

(* ── Headlines ──────────────────────────────────────────────────────────── *)

(* Every collection operation satisfies prefix coverage. The reads after
   the commit are covered by the reservations that the staged part made for
   them. *)
Theorem operation_trace_covered : forall x, covered (operation_trace x).
Proof.
  intros x. unfold covered, operation_trace.
  pose proof (staged_totals x) as totals.
  apply covered_from_app; [apply staged_covered | lia |].
  apply reads_only_covered.
  - unfold after_commit_trace. rewrite total_cons, after_rounds_reserved. reflexivity.
  - unfold after_commit_trace. rewrite total_cons, after_rounds_read. simpl. lia.
Qed.

(* The trace after the commit reserves nothing. *)
Theorem reservations_precede_commit : forall x,
  total reserved_units (after_commit_trace x) = 0.
Proof.
  intros x. unfold after_commit_trace.
  rewrite total_cons, after_rounds_reserved. reflexivity.
Qed.

(* A budget below the reservations of an operation is exceeded by a
   reservation of the staged part. That reservation fails, so the operation
   aborts before the commit and leaves no effect. *)
Theorem cut_rejects_before_commit : forall x budget,
  budget < total reserved_units (operation_trace x) ->
  exists prefix units suffix,
    staged_trace x = prefix ++ Reserve units :: suffix /\
    total reserved_units prefix <= budget /\
    budget < total reserved_units prefix + units.
Proof.
  intros x budget exceeds.
  unfold operation_trace in exceeds.
  rewrite total_app, reservations_precede_commit in exceeds.
  apply first_crossing. lia.
Qed.

Lemma sum_permutation : forall amounts amounts',
  Permutation amounts amounts' -> sum amounts = sum amounts'.
Proof.
  intros amounts amounts' perm.
  induction perm; rewrite ?sum_cons in *; lia.
Qed.

Lemma length_tl : forall (A : Type) (l : list A), length (tl l) = length l - 1.
Proof. intros A l. destruct l; simpl; lia. Qed.

Lemma sum_constant : forall (A : Type) (c : nat) (l : list A),
  sum (map (fun _ => c) l) = c * length l.
Proof.
  intros A c l. induction l as [| a rest IH]; [cbn [map length]; rewrite sum_nil; lia |].
  cbn [map length]. rewrite sum_cons, IH. lia.
Qed.

Lemma scan_reserved : forall side head,
  total reserved_units (scan side head) =
  buffer (length side) + sum (map envelope side)
  + compare * (length side - 1) + payload head.
Proof.
  intros side head. unfold scan.
  rewrite !total_app, !steps_reserved, sum_constant, length_tl.
  unfold total, step. simpl. lia.
Qed.

(* The charge of a scan does not depend on the list order of the side. *)
Theorem scan_charge_permutation_invariant : forall side side' head,
  Permutation side side' ->
  total reserved_units (scan side' head) = total reserved_units (scan side head) /\
  total read_units (scan side' head) = total read_units (scan side head).
Proof.
  intros side side' head perm.
  pose proof (scan_balanced side head) as [_ equal].
  pose proof (scan_balanced side' head) as [_ equal'].
  assert (same : total reserved_units (scan side' head) = total reserved_units (scan side head)).
  { rewrite !scan_reserved.
    rewrite (Permutation_length perm).
    rewrite (sum_permutation (map envelope side) (map envelope side'))
      by (apply Permutation_map; exact perm).
    reflexivity. }
  split; [exact same | lia].
Qed.

(* Two rounds that differ only in the list order of their sides. *)
Definition reordered (r r' : round) : Prop :=
  Permutation (side r) (side r') /\ head r' = head r /\ decoded r' = decoded r /\
  matcher r' = matcher r /\ fires r' = fires r /\ unwrap r' = unwrap r /\
  staging r' = staging r.

Lemma round_reserved_reordered : forall r r',
  reordered r r' ->
  total reserved_units (round_staged r') = total reserved_units (round_staged r).
Proof.
  intros r r' [perm [hd [dec [mat [fire [unw stg]]]]]].
  unfold round_staged. rewrite hd, dec, mat, fire, unw, stg.
  rewrite !total_app.
  destruct (scan_charge_permutation_invariant (side r) (side r') (head r) perm) as [same _].
  lia.
Qed.

(* The charge of an operation does not depend on the list order of the
   staged view: two operations whose rounds differ only in the order of their
   sides reserve and read the same units. *)
Theorem operation_charge_permutation_invariant : forall x x',
  setup x' = setup x -> bump x' = bump x -> prepare x' = prepare x ->
  outcome x' = outcome x -> Forall2 reordered (rounds x) (rounds x') ->
  total reserved_units (operation_trace x') = total reserved_units (operation_trace x) /\
  total read_units (operation_trace x') = total read_units (operation_trace x).
Proof.
  intros x x' same_setup same_bump same_prepare same_outcome rounds_reordered.
  assert (rounds_reserved :
    total reserved_units (flat_map round_staged (rounds x')) =
    total reserved_units (flat_map round_staged (rounds x))).
  { induction rounds_reordered as [| r r' rest rest' one others IH]; [reflexivity |].
    change (flat_map round_staged (r' :: rest'))
      with (round_staged r' ++ flat_map round_staged rest').
    change (flat_map round_staged (r :: rest))
      with (round_staged r ++ flat_map round_staged rest).
    rewrite !total_app, (round_reserved_reordered r r' one). lia. }
  assert (prepaid_same : sum (map prepaid (rounds x')) = sum (map prepaid (rounds x))).
  { clear rounds_reserved.
    induction rounds_reordered as [| r r' rest rest' one others IH]; [reflexivity |].
    destruct one as [_ [_ [_ [_ [fire [unw _]]]]]].
    cbn [map]. rewrite !sum_cons, IH. unfold prepaid. rewrite fire, unw. reflexivity. }
  assert (reserved_same :
    total reserved_units (operation_trace x') = total reserved_units (operation_trace x)).
  { unfold operation_trace.
    rewrite !total_app, !reservations_precede_commit.
    unfold staged_trace.
    rewrite !total_app, !steps_reserved, same_setup, same_bump, same_prepare, same_outcome,
      rounds_reserved.
    reflexivity. }
  split; [exact reserved_same |].
  unfold operation_trace in *.
  rewrite !total_app in *.
  pose proof (staged_totals x) as totals.
  pose proof (staged_totals x') as totals'.
  unfold after_commit_trace.
  rewrite !total_cons, !after_rounds_read. simpl.
  rewrite reservations_precede_commit, reservations_precede_commit in reserved_same.
  rewrite same_outcome, prepaid_same in *.
  lia.
Qed.

Lemma map_constant_length : forall (A B : Type) (c : B) (l l' : list A),
  length l = length l' -> map (fun _ => c) l = map (fun _ => c) l'.
Proof.
  intros A B c l. induction l as [| a rest IH]; intros l' same; destruct l' as [| a' rest'];
    simpl in *; try discriminate; [reflexivity |].
  f_equal. apply IH. lia.
Qed.

(* A scan reads each envelope and copies only the head, so the payloads of
   the other entries do not change it. *)
Theorem scan_charge_independent_of_unread_payloads : forall side side' head,
  map envelope side' = map envelope side ->
  scan side' head = scan side head.
Proof.
  intros side side' head same_envelopes.
  unfold scan.
  assert (same_length : length side' = length side).
  { rewrite <- (length_map envelope side'), <- (length_map envelope side), same_envelopes.
    reflexivity. }
  assert (same_tail : map (fun _ => compare) (tl side') = map (fun _ => compare) (tl side)).
  { apply map_constant_length. rewrite !length_tl, same_length. reflexivity. }
  rewrite same_length, same_envelopes, same_tail. reflexivity.
Qed.

(* ── Negative controls ──────────────────────────────────────────────────── *)

(* The P4 scan before decision 42: the decode of each envelope and the copy
   of the head were reads with no reservation (the matcher reserved only its
   own reads). *)
Definition p4_scan (side : list entry) (head : entry) : list event :=
  map Read (map envelope side) ++ [Read (payload head)].

Lemma read_list_total : forall amounts,
  total read_units (map Read amounts) = sum amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  cbn [map]. rewrite total_cons, IH, sum_cons. reflexivity.
Qed.

Lemma reserve_free_reads : forall amounts,
  total reserved_units (map Read amounts) = 0.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  cbn [map]. rewrite total_cons, IH. reflexivity.
Qed.

(* The P4 scan satisfies prefix coverage only when it does no work of its
   own: every envelope decode and the head copy are zero. *)
Theorem p4_scan_covered_only_if_free : forall side head,
  covered (p4_scan side head) ->
  sum (map envelope side) = 0 /\ payload head = 0.
Proof.
  intros side head covered_scan.
  pose proof (covered_scan (p4_scan side head) [] (eq_sym (app_nil_r _))) as whole.
  unfold p4_scan in whole.
  rewrite !total_app, read_list_total, reserve_free_reads in whole.
  unfold total in whole. simpl in whole. lia.
Qed.

(* Without the prepaid reservations, the staged trace is balanced, so the
   reads of the outcome after the commit exceed the reservations whenever the
   outcome reads anything. *)
Definition unprepaid_round (r : round) : list event :=
  scan (side r) (head r)
  ++ flat_map step (decoded r)
  ++ flat_map step (matcher r)
  ++ flat_map step (staging r).

Definition unprepaid_trace (x : operation) : list event :=
  flat_map step (setup x) ++ flat_map step (bump x) ++ flat_map step (prepare x)
  ++ flat_map unprepaid_round (rounds x) ++ after_commit_trace x.

Lemma unprepaid_rounds_balanced : forall rs,
  total read_units (flat_map unprepaid_round rs) =
  total reserved_units (flat_map unprepaid_round rs).
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map unprepaid_round (r :: rest))
    with (unprepaid_round r ++ flat_map unprepaid_round rest).
  pose proof (scan_balanced (side r) (head r)) as [_ scan_equal].
  assert (round_equal :
    total read_units (unprepaid_round r) = total reserved_units (unprepaid_round r)).
  { unfold unprepaid_round. rewrite !total_app, !steps_read, !steps_reserved. lia. }
  rewrite !total_app. lia.
Qed.

Theorem unprepaid_trace_uncovered : forall x,
  0 < outcome x -> ~ covered (unprepaid_trace x).
Proof.
  intros x positive covered_trace.
  pose proof (covered_trace (unprepaid_trace x) [] (eq_sym (app_nil_r _))) as whole.
  unfold unprepaid_trace, after_commit_trace in whole.
  pose proof (unprepaid_rounds_balanced (rounds x)).
  rewrite !total_app, !steps_read, !steps_reserved, !total_cons,
    after_rounds_read, after_rounds_reserved in whole.
  simpl in whole. lia.
Qed.

End Agent.

Print Assumptions operation_trace_covered.
Print Assumptions reservations_precede_commit.
Print Assumptions cut_rejects_before_commit.
Print Assumptions scan_charge_permutation_invariant.
Print Assumptions operation_charge_permutation_invariant.
Print Assumptions scan_charge_independent_of_unread_payloads.
Print Assumptions p4_scan_covered_only_if_free.
Print Assumptions unprepaid_trace_uncovered.
