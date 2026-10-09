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
   step                     reified/costs.rs:98-103, 107-112, 124-127
                            (ScanMeter::inspect, reserve_copy, copy:
                            reserve, then work)
   scan                     spaces/generic.rs:1001-1017, 1021-1034, 1038-1049
                            (Cx::scan_head, Cx::head_waiter, Cx::head_datum:
                            the buffer, one envelope decode for each entry,
                            the head copy) and
                            spaces/disciplines/queue.rs:41-59, 88-123
                            (head_metered: one comparison for each entry
                            after the first)
   round_staged             spaces/generic.rs:1723-1791, 1800-1884
                            (offer_head and receive_head: the scan, the
                            payload decode of a receive, the metered match,
                            the prepaid unwrap, the staged fire). For a peek
                            receive of a non-persistent record, the staging
                            of the round also holds the re-store of decision
                            45: the reserved copy of the record and the
                            steps of Cx::restore (spaces/generic.rs:1253-1268:
                            the channel copy, Introduction::for_record and
                            register_introduction), each self-metered
   round_after_commit       spaces/generic.rs:1991-2080 (outcome: the moves of
                            into_continuation, prepaid by
                            spaces/record.rs:750-764)
   operation_trace          spaces/generic.rs:2103-2317, 2544-2714
                            (begin_collection and run_with: the channels, the
                            config, the staged list, the matcher handle, the
                            introduction, the bump, the record, the rounds and
                            the prepaid outcome buffer). The re-entry checks of
                            decision 43 are self-metered: the two comparisons of
                            operation_ord (spaces/generic.rs:1915-1943), and the
                            scan of Cx::head_ord and Cx::at_head
                            (spaces/generic.rs:1053-1073, 1090-1111: the buffer and
                            the envelope decodes, with no copy) with the
                            comparison of the two orders and
                            QueueDiscipline::precedes
                            (spaces/disciplines/queue.rs:65-74)
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
   - The legacy charges of decision 45 (Option P, LegacyChain in
     reified/legacy_charges.rs) walk the encoded lengths of the staged
     operations. The chain runs only in a legacy deploy, which has no
     host-work budget, as ChargingRSpace runs its walks. So these reads
     are not events of the model.

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

(* ═══════════════════════════════════════════════════════════════════════════
   Appendix (P5, the engine spine S1): the primitives of the engine

   The spine S1 of P5 turns the queue agent into the engine of every space
   family. It adds four primitives to the work of a collection operation:
   - the growth of a staged list (push_reserved): a list at its capacity
     reserves its doubled buffer, and then it grows. The staged list of a
     plan and its list of prefix counts both grow this way, so a plan that
     stages more than MAX_STAGED operations pays for each growth first;
   - a group fire over [D; n] (Cx::fire_receive_group): the copies of the
     candidates of the other members of the group, each reserved;
   - the prefix outcome: after the commit, the outcome of a fire that
     dispatches a prefix moves its continuation, as every fire does, and
     truncates its results, which reads nothing;
   - the capacity check (Cx::check_capacity): one comparison, reserved
     before a store or a park.
   The headline engine_primitives_trace_covered states that an operation
   with these primitives keeps the metering rule of decision 42: its trace
   is covered, nothing is reserved after the commit, a budget below its
   reservations is exceeded before the commit, and an operation that stops
   early (for example a CellOccupied abort at its capacity check) has a
   covered trace.

   Traceability: Rocq definition → Rust code (spaces/generic.rs at the S1
   commit 21b2b27fd)
   grown, grow_step, growth push_reserved (1084-1101): the doubled capacity,
                            at least 1, reserved with reserve_buffer, then
                            try_reserve_exact
   engine_round             one round of a plan of S1: the round of the P4
                            model and its S1 primitives
   er_capacity              Cx::check_capacity (758-773): the comparison of
                            the size of a side with D::CAPACITY
   er_group                 Cx::fire_receive_group (959-998): the reserved
                            copies of the candidates of the other members
                            (the plan builds them)
   er_pushes, er_staged_*   Cx::push_staged (791-795) for the staged
                            operations of the round
   er_prefix, er_prefix_*   the count of a prefix fire joins Plan::prefixes
                            with push_reserved (994)
   er_results               the results of a group receipt, which outcome
                            truncates (1393-1465: results.truncate(binds))
   engine_round_after       outcome after the commit: into_continuation of
                            each fire, prepaid by prepay_continuation_unwrap
   eo_outcome               prepay_outcome (1385-1391): the buffer of the
                            fires of the outcome

   Abstraction. The segments of one round are balanced or prepaid, so their
   order inside the round does not change coverage. The model puts the S1
   primitives after the P4 part of the round.

   Rust correspondence: rholang/tests/reified_engine_props.rs
   (prop_engine_primitives_reserve_before_they_read); the S1 unit test
   space_engine_staged_list_growth_is_reserved_before_it_grows
   (spaces/generic.rs).
   ═══════════════════════════════════════════════════════════════════════ *)

(* The capacity of a list after push_reserved grows it at the capacity cap. *)
Definition grown (cap : nat) : nat := Nat.max (2 * cap) 1.

Section Engine.

Variable buffer : nat -> nat.
Variable compare : nat.

(* push_reserved at the capacity: reserve the doubled buffer, then grow. *)
Definition grow_step (units : nat) : list event := [Reserve units; Read units].

(* n pushes onto a list of capacity cap and length len. *)
Fixpoint growth (n cap len : nat) : list event :=
  match n with
  | 0 => []
  | S n' =>
      if len =? cap then grow_step (buffer (grown cap)) ++ growth n' (grown cap) (S len)
      else growth n' cap (S len)
  end.

(* One round of a plan of S1: the round of the P4 model and the primitives
   of S1. *)
Record engine_round : Type := mkEngineRound {
  er_round : round;          (* the scan, the decodes, the matcher, the fire and its copies *)
  er_capacity : bool;        (* a capacity check before a store or a park *)
  er_group : list nat;       (* the reserved copies of the other members of a group fire *)
  er_pushes : nat;           (* the staged operations that the round pushes *)
  er_staged_cap : nat;       (* the capacity of the staged list before the round *)
  er_staged_len : nat;       (* the length of the staged list before the round *)
  er_prefix : bool;          (* the fire dispatches a prefix of its results *)
  er_prefix_cap : nat;       (* the capacity of the list of prefix counts *)
  er_prefix_len : nat;       (* the length of the list of prefix counts *)
  er_results : nat           (* the units of the results of the receipt of the fire *)
}.

Definition engine_round_staged (r : engine_round) : list event :=
  round_staged buffer compare (er_round r)
  ++ (if er_capacity r then step compare else [])
  ++ flat_map step (er_group r)
  ++ growth (er_pushes r) (er_staged_cap r) (er_staged_len r)
  ++ (if er_prefix r then growth 1 (er_prefix_cap r) (er_prefix_len r) else []).

(* The outcome of a fire after the commit: the moves of its continuation,
   which the round prepaid. A prefix fire truncates its results, which reads
   nothing. *)
Definition engine_round_after (r : engine_round) : list event :=
  if fires (er_round r) then [Read (unwrap (er_round r))] else [].

Record engine_operation : Type := mkEngineOperation {
  eo_setup : list nat;           (* the channels, the config, the key, param and query checks *)
  eo_bump : list nat;            (* the counter bump, or nothing without a counter *)
  eo_prepare : list nat;         (* the record, its identity, the re-entry checks *)
  eo_rounds : list engine_round; (* the rounds of the plan *)
  eo_outcome : nat               (* the buffer of the fires of the outcome *)
}.

Definition engine_staged (x : engine_operation) : list event :=
  flat_map step (eo_setup x) ++ flat_map step (eo_bump x) ++ flat_map step (eo_prepare x)
  ++ flat_map engine_round_staged (eo_rounds x) ++ [Reserve (eo_outcome x)].

Definition engine_after_commit (x : engine_operation) : list event :=
  Read (eo_outcome x) :: flat_map engine_round_after (eo_rounds x).

Definition engine_trace (x : engine_operation) : list event :=
  engine_staged x ++ engine_after_commit x.

Lemma grow_step_balanced : forall units, balanced (grow_step units).
Proof. intros units. exact (steps_balanced [units]). Qed.

Lemma growth_balanced : forall n cap len, balanced (growth n cap len).
Proof.
  induction n as [| n IH]; intros cap len.
  - split; [intros slack; apply covered_from_nil | reflexivity].
  - cbn [growth]. destruct (len =? cap).
    + apply balanced_app; [apply grow_step_balanced | apply IH].
    + apply IH.
Qed.

Lemma engine_extras_balanced : forall r,
  balanced ((if er_capacity r then step compare else [])
            ++ flat_map step (er_group r)
            ++ growth (er_pushes r) (er_staged_cap r) (er_staged_len r)
            ++ (if er_prefix r then growth 1 (er_prefix_cap r) (er_prefix_len r) else [])).
Proof.
  intros r. apply balanced_app; [destruct (er_capacity r); [exact (steps_balanced [compare]) |
    split; [intros slack; apply covered_from_nil | reflexivity]] |].
  apply balanced_app; [apply steps_balanced |].
  apply balanced_app; [apply growth_balanced |].
  destruct (er_prefix r); [apply growth_balanced | split; [intros slack; apply covered_from_nil | reflexivity]].
Qed.

Lemma engine_round_covered : forall slack r, covered_from slack (engine_round_staged r).
Proof.
  intros slack r. unfold engine_round_staged.
  pose proof (round_staged_totals buffer compare (er_round r)) as totals.
  destruct (engine_extras_balanced r) as [extras_covered _].
  apply covered_from_app; [apply round_staged_covered | lia | apply extras_covered].
Qed.

Lemma engine_round_totals : forall r,
  total read_units (engine_round_staged r) + prepaid (er_round r) =
  total reserved_units (engine_round_staged r).
Proof.
  intros r. unfold engine_round_staged.
  pose proof (round_staged_totals buffer compare (er_round r)) as totals.
  destruct (engine_extras_balanced r) as [_ extras_equal].
  rewrite !total_app. rewrite !total_app in extras_equal. lia.
Qed.

Lemma engine_rounds_covered : forall slack rs,
  covered_from slack (flat_map engine_round_staged rs).
Proof.
  intros slack rs. revert slack.
  induction rs as [| r rest IH]; intros slack; [apply covered_from_nil |].
  change (covered_from slack (engine_round_staged r ++ flat_map engine_round_staged rest)).
  pose proof (engine_round_totals r) as totals.
  apply covered_from_app; [apply engine_round_covered | lia | apply IH].
Qed.

Lemma engine_rounds_totals : forall rs,
  total read_units (flat_map engine_round_staged rs) + sum (map (fun r => prepaid (er_round r)) rs) =
  total reserved_units (flat_map engine_round_staged rs).
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map engine_round_staged (r :: rest))
    with (engine_round_staged r ++ flat_map engine_round_staged rest).
  pose proof (engine_round_totals r).
  cbn [map]. rewrite !total_app, sum_cons. lia.
Qed.

Lemma engine_after_rounds_read : forall rs,
  total read_units (flat_map engine_round_after rs) = sum (map (fun r => prepaid (er_round r)) rs).
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map engine_round_after (r :: rest))
    with (engine_round_after r ++ flat_map engine_round_after rest).
  cbn [map]. rewrite total_app, IH, sum_cons. unfold engine_round_after, prepaid.
  destruct (fires (er_round r)); unfold total; simpl; lia.
Qed.

Lemma engine_after_rounds_reserved : forall rs,
  total reserved_units (flat_map engine_round_after rs) = 0.
Proof.
  induction rs as [| r rest IH]; [reflexivity |].
  change (flat_map engine_round_after (r :: rest))
    with (engine_round_after r ++ flat_map engine_round_after rest).
  rewrite total_app, IH. unfold engine_round_after.
  destruct (fires (er_round r)); unfold total; simpl; lia.
Qed.

Lemma engine_staged_totals : forall x,
  total read_units (engine_staged x) + eo_outcome x + sum (map (fun r => prepaid (er_round r)) (eo_rounds x)) =
  total reserved_units (engine_staged x).
Proof.
  intros x. unfold engine_staged.
  pose proof (engine_rounds_totals (eo_rounds x)).
  rewrite !total_app, !steps_read, !steps_reserved, reserve_one_read, reserve_one_reserved.
  lia.
Qed.

Lemma engine_staged_covered : forall slack x, covered_from slack (engine_staged x).
Proof.
  intros slack x.
  assert (shape : engine_staged x =
    flat_map step (eo_setup x ++ eo_bump x ++ eo_prepare x) ++
    (flat_map engine_round_staged (eo_rounds x) ++ [Reserve (eo_outcome x)])).
  { unfold engine_staged. rewrite !flat_map_app, <- !app_assoc. reflexivity. }
  rewrite shape.
  pose proof (steps_balanced (eo_setup x ++ eo_bump x ++ eo_prepare x)) as [steps_cov steps_equal].
  pose proof (engine_rounds_totals (eo_rounds x)) as round_totals.
  apply covered_from_app; [apply steps_cov | lia |].
  apply covered_from_app; [apply engine_rounds_covered | lia | apply reserve_covered].
Qed.

Lemma engine_after_commit_reserved : forall x, total reserved_units (engine_after_commit x) = 0.
Proof.
  intros x. unfold engine_after_commit. rewrite total_cons, engine_after_rounds_reserved. reflexivity.
Qed.

Lemma covered_prefix : forall pre post, covered (pre ++ post) -> covered pre.
Proof.
  intros pre post H prefix suffix split. subst pre.
  exact (H prefix (suffix ++ post) (eq_sym (app_assoc prefix suffix post))).
Qed.

Lemma engine_trace_covered : forall x, covered (engine_trace x).
Proof.
  intros x. unfold covered, engine_trace.
  pose proof (engine_staged_totals x) as totals.
  apply covered_from_app; [apply engine_staged_covered | lia |].
  apply reads_only_covered.
  - apply engine_after_commit_reserved.
  - unfold engine_after_commit. rewrite total_cons, engine_after_rounds_read. simpl. lia.
Qed.

(* Decision 42 for the primitives of the engine spine S1.
   1. The growth of a staged list reserves each doubled buffer before the
      list grows: every growth is a balanced segment.
   2. An operation with capacity checks, group fires, prefix fires and list
      growth keeps prefix coverage, its reads after the commit included.
   3. The trace after the commit reserves nothing: a prefix outcome only
      moves what its fire prepaid.
   4. A budget below the reservations of such an operation is exceeded by a
      reservation of the staged part, so the operation fails before the
      commit and has no effect.
   5. An operation that stops early, at any point of its staged part (a
      CellOccupied abort at a capacity check, or a host-work rejection), has
      a covered trace up to that point. *)
Theorem engine_primitives_trace_covered :
  (forall n cap len, balanced (growth n cap len)) /\
  (forall x, covered (engine_trace x)) /\
  (forall x, total reserved_units (engine_after_commit x) = 0) /\
  (forall x budget, budget < total reserved_units (engine_trace x) ->
     exists prefix units suffix,
       engine_staged x = prefix ++ Reserve units :: suffix /\
       total reserved_units prefix <= budget /\
       budget < total reserved_units prefix + units) /\
  (forall x pre post, engine_staged x = pre ++ post -> covered pre).
Proof.
  split; [exact growth_balanced |].
  split; [exact engine_trace_covered |].
  split; [exact engine_after_commit_reserved |].
  split.
  { intros x budget exceeds. unfold engine_trace in exceeds.
    rewrite total_app, engine_after_commit_reserved in exceeds.
    apply first_crossing. lia. }
  intros x pre post shape. apply (covered_prefix pre post).
  rewrite <- shape. intros prefix suffix split.
  exact (engine_staged_covered 0 x prefix suffix split).
Qed.

End Engine.

Print Assumptions engine_primitives_trace_covered.
