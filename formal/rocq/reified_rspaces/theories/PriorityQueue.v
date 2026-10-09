(* ═══════════════════════════════════════════════════════════════════════════
   PriorityQueue.v — the priority queue discipline and its settle (P5b)

   This module models the priority queue discipline of the native spaces
   (plan decisions 11 and 38). It also models the settle of a priority queue
   (F2 of decision 39, the P5 design v3, section A.8). A priority queue keeps its
   records in levels. The levels ascend, and the unprioritized level comes
   after every priority level (F6). The waiting continuations wait in lanes:
   the waiter queue of a level p, and the unrestricted queue. Each level and
   each lane is a FIFO queue. Its head is its entry with the least
   (ord, bincode bytes), and only heads interact (decision 11). The waiter
   queue of the level p sees the head of the level p. The unrestricted queue
   sees the heads of all levels in ascending level order. One counter serves
   the whole priority queue (decision 31), so every order is unique.

   The plans of an operation are offer_pq and receive_pq of priority_queue.rs.
     PQ-OFFER    A fresh datum joins the tail of its level, so it plans only
                 on an empty level. A re-issued persistent datum plans when
                 its level is empty or its first-round order precedes the
                 order of the head (decision 43). The candidates are the head
                 of the waiter queue of its level and the head of the
                 unrestricted queue. The older candidate is tested first. The
                 first candidate that fits (the pattern, then the guard)
                 fires, and a refusal moves on (decisions 37 and 38). A fire
                 that leaves the datum unspent offers it again (decision 30).
                 With no fit, the datum is stored.
     PQ-RECEIVE  A fresh continuation joins the tail of its lane, so it plans
                 only on an empty lane. A re-issued persistent continuation
                 plans when its lane is empty or its order precedes the order
                 of the head. The receive binds the first level head, in
                 ascending level order, that its lane sees and whose payload
                 matches its pattern. It checks its guard once, on that head,
                 and a refusal parks it (decision 38). A peek of a record
                 that is not persistent stores the record again at once
                 (decision 45, Rule E).
   One space transaction makes at most cap fires, its settles included
   (revision I-21, decision 49). The settle before the plan starts the count
   at 0. open_plan opens no plan at the cap. The plan continues the count,
   and the settle after the plan continues it again. A settle job starts its
   own count at 0.
   The settle loop is the loop of settle.rs with PriorityQueueView.
     loop: F := the fireable waiter-lane head with the least order. F binds
           the first level head that the waiter sees and matches, when the
           guard admits that head. The record in flight counts as the head
           of its level.
           With no F: land the record in flight and stop (Quiescent).
           When F binds a stored record while a record is in flight: land
           the record in flight first, and plan again (settle.rs).
           At cap fires of the transaction: land and re-issue.
           Lift the record when it is stored and not persistent, and offer
           it again to the waiter. after_fire decides. Done or Continue: go
           on (a record that a peek left stays in flight). Divergent or
           RoundCap: land and re-issue.
   Strict confluence fails for a priority queue (readiness review S4), so
   the settle fires the oldest fireable head first (headlines 6 and 7).

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md, sections A.8
     (Settle, and PQ (decision 38)), A.12 (the PQ row) and A.13 (the P5b
     row).
   - Design v2: p5-design-v2.md, section 3.1 (the priority queue, the
     confluence example S4, and the termination measure).
   - Review: p5-readiness-review.md, S4.
   - Plan: decisions 11 (head-only), 30 (the re-offer of a datum that a peek
     waiter left) and 31 (every operation bumps the counter). Decisions 37
     (the current guard rule), 38 (the lanes of a priority queue) and 42
     (reserve every read and copy). Decisions 43 (a re-issued persistent
     operation keeps its first-round order), 45 (Rule E) and 49 (revision
     I-21: one fire count for each transaction). F2 (the settle), F6 (the
     unprioritized level is the lowest) and I-10 (no storage for each
     level).
   - Code: the PQ batch of wave W2 of P5 (spaces/disciplines/priority_queue.rs)
     on spaces/settle.rs and spaces/generic.rs with revision I-21. The line
     numbers below are the line numbers of that batch.
   - The port: the legacy Collections/PriorityQueue.v gives headlines 2, 14
     and 15. It has the valid and the invalid priorities, the default level
     as a hypothesis, and the level scan.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (rholang/src/rust/interpreter)
   ─────────────────────────────────────────────────────────────────────────
   below, least_from, least  spaces/disciplines/priority_queue.rs:243-250
                             and 355-397 (lane_heads with head_metered of
                             the queue on each pair: the least (ord, bytes),
                             the earlier entry on equal keys)
   lvl_class, lvl_index,     spaces/disciplines/priority_queue.rs:119-155
   lvl_le                    (Level and its derived order: Priority(p)
                             ascending, then Unprioritized)
   sees                      spaces/disciplines/priority_queue.rs:186-192
                             (Lane::sees)
   record_head, waiter_head  spaces/disciplines/priority_queue.rs:400-411
                             (record_heads and waiter_heads)
   view                      spaces/disciplines/priority_queue.rs:446-506
                             (record_head_list: the record in flight is the
                             head of its level when its order is the least)
   first_match               spaces/disciplines/priority_queue.rs:513-539
                             (the first level head, in ascending level
                             order, that the lane sees and that matches)
   fireable                  spaces/disciplines/priority_queue.rs:978-1072
                             (first_match, then Cx::admits once)
   fire_candidates, oldest,  spaces/disciplines/priority_queue.rs:978-1072
   next_fire                 (PriorityQueueView::next_fire: the fireable
                             waiter-lane head with the least order)
   lands_first               spaces/settle.rs:425-430 (a fire of a stored
                             record while a record is in flight)
   settle_loop, lift_ops,    spaces/settle.rs:404-498, 508-513 and 519-563
   flying, flight_after,     (settle, land and lift)
   land
   at_cap                    spaces/settle.rs:431 (the cap check of the
                             transaction at the top of a settle round)
   settle_of, settle_at      spaces/settle.rs:569-582, 588-601 and 606-616
                             (settle_before_plan, settle_after_plan and
                             settle_job)
   after_fire, cap           reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,
                             FireRounds::after_fire)
   cand1, cand2              spaces/disciplines/priority_queue.rs:149-154
                             (Level::candidate_lanes) and 542-594 (the
                             lookup of each candidate lane head)
   offer_pick                spaces/disciplines/priority_queue.rs:545-597
                             (offer_target: the older head first, and each
                             refusal marks its lane)
   fits_datum                spaces/generic.rs:823-830 (Cx::matches)
   offer_loop                spaces/disciplines/priority_queue.rs:765-824
                             (offer_pq)
   receive_loop,             spaces/disciplines/priority_queue.rs:830-933 and
   consume_fire              929-938 (receive_pq and restore_peeked) and
                             spaces/generic.rs:420-427 (Discipline::restore)
   fresh_offer,              spaces/disciplines/priority_queue.rs:254-268 and
   fresh_receive,            416-437 (reaches_head, precedes and
   reenter_offer,            at_lane_head: a fresh element joins the tail of
   reenter_receive           its lane, and a re-entry precedes with <)
   with_settles,             spaces/generic.rs:1535-1548 (open_plan) and
   plan_at_cap               spaces/disciplines/priority_queue.rs:319-345
                             (the settle hooks)
   carried, next_round       spaces/settle.rs:643-675 and 681-705, and
                             spaces/disciplines/priority_queue.rs:816 and 925
                             (reissued_ord: persistent.then_some(ord))
   produce, consume,         spaces/generic.rs:2328-2487 (run_with) and
   bumped, bump_ord          spaces/counter.rs:165-245 (bump)
   fires (the fire count)    spaces/generic.rs:686, 751 and 2400 (Plan.rounds,
                             Cx::rounds, and the FireRounds of a transaction)
   apply, run, drop_key      the staged view of the substrate
                             (space_store_handle.rs, stage_produce and
                             stage_consume), as in StackDiscipline.v
   priority_admitted,        spaces/disciplines/priority_queue.rs:275-295 and
   produce_checked           658-738 (check_values, priority_of,
                             priorities_bound, check_priority, send_level)
   size                      the staged view: records and waiters only (I-10)
   lane_scan, head_test,     spaces/disciplines/priority_queue.rs:355-397,
   settle_round,             443-503, 510-536, 542-594, 756-815, 821-924 and
   receive_round,            969-1063, and spaces/settle.rs:404-498 and
   offer_round               519-563 (the scans, the decodes, the matches,
                             the copies, the prepaid unwrap and the fires)
   produce_without_settle,   the Unsafe control PqNoSettleUnsafe of
   consume_without_settle    PriorityQueue.tla (no settle hooks)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - spatial p v is the metered spatial match (Cx::bind), and admits g v is
     the guard over the match (Cx::admits). They are section variables, so
     every theorem holds for every matcher and every guard.
   - cap is FIRE_ROUND_CAP. It is a section variable, so every theorem holds
     for every cap. Headlines 7 and 13 instantiate 64.
   - A level is an option nat: Some p is Level::Priority(p), and None is
     Level::Unprioritized. A lane is an option nat: Some p is Lane::Level(p),
     and None is Lane::Any. The function check_values admits only
     priorities p >= 0, so the levels of the code are natural numbers
     (headline 14).
   - rank is the position of the bincode bytes of an entry in their order.
     Two entries with one (ord, rank) are byte-identical. The code compares
     the record in flight and the candidates by order only. With distinct
     orders (wf), the two comparisons agree.
   - The staged view of a collection is a record list and a waiter list. The
     state after staged operations is the fold of apply over them (run). The
     bump changes the counter only (bumped).
   - A removal takes the first entry with the key of the removed entry
     (drop_key). With distinct orders, the key names the entry.
   - The model omits the Inconsistent errors of record_head_list and
     at_lane_head (equal orders). The premise wf' of decision 43 excludes
     equal orders, and the headlines that need it assume it.
   - A settle fire binds one record on one channel.
   - The identities of K_lift are not modeled (StackDiscipline.v explains
     why, and the Rust tests of settle.rs pin them).
   - One fire count runs through each transaction, as in the code (I-21). The
     plan-open check (plan_at_cap) and the cap check of a settle round
     (at_cap) are two definitions, as open_plan and settle make two checks.
   - The fuel of a settle is S (2 * cap). Each fire takes one round, and a
     landing before a fire of a stored record takes one round. The last
     round ends the settle.
   - produce_checked models the check of the priority param of a send. The
     query param of a receive has the same check (check_priority).
   - Section 19 counts one host-work dimension. Each dimension satisfies the
     rule alone.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. pq_level_order                     the levels ascend in a total order,
                                          and a receive binds the first
                                          matching visible level head
                                          (PqLevelOrder)
    2. pq_unprioritized_lowest            the unprioritized level is the
                                          lowest level (F6)
    3. pq_fifo_within_level               each lane head is its least
                                          (ord, rank), and every fire binds
                                          heads (FIFO, decision 11)
    4. pq_restricted_waiter_in_level_queue
                                          a receive with priority p waits in
                                          the waiter queue of the level p and
                                          fires only on that level
    5. pq_unrestricted_first_matching_level
                                          an unrestricted receive takes the
                                          first matching level head and
                                          checks its guard once (decision 38)
    6. pq_settle_oldest_first             the settle fires the oldest
                                          fireable head, and a plan fires the
                                          older fitting candidate
                                          (PqOldestFirst)
    7. pq_settle_order_matters            strict confluence fails: two fire
                                          orders end in different states (the
                                          computed example S4)
    8. pq_produce_preserves_inv           a produce that completes in its
                                          round leaves INV (PqNoPendingMatch)
    9. pq_consume_preserves_inv           a consume that completes in its
                                          round leaves INV
   10. pq_head_blocks_within_level        a head that does not match blocks
                                          its lane (for a cap above 0)
   11. pq_settle_terminates               a settle that starts after f fires
                                          of its transaction fires at most
                                          cap - f times. It fires at most μ
                                          times, plus one divergent fire. Every
                                          transaction fires at most cap
                                          times, and a transaction that
                                          re-issues at the cap fired exactly
                                          cap times (I-21, TxnFiresBounded)
   12. pq_plan_permutation_invariant      the plans ignore the list order
   13. pq_without_settle_breaks_inv       without the settle INV fails (two
                                          computed counterexamples)
   14. pq_priority_domain                 a priority is admitted exactly
                                          below the bound, and an
                                          inadmissible send changes nothing
   15. pq_no_level_storage                a level exists exactly when a
                                          record holds it, and an operation
                                          adds at most one entry (I-10)
   16. pq_reissue_takes_level_fifo_place  a re-issued send takes its FIFO
                                          place in its level and keeps its
                                          first-round order (decision 43)
   17. pq_reentered_waiter_keeps_seniority
                                          a re-entered receive parks at its
                                          FIFO place and keeps its
                                          first-round order (decision 43)
   18. pq_peek_restores_head              a peek re-stores the head of its
                                          level and starts no settle
                                          (decision 45, Rule E)
   19. level_scan_trace_covered           the level scans reserve their reads
                                          before they make them (DR-88)
   20. pq_settle_keeps_unspent_records    a settle loses no record, from
                                          every fire count (NoRecordLost)

   The premise "a cap above 0" of headlines 10, 16, 17 and 18 comes from
   open_plan. With a cap of 0, no plan opens (FIRE_ROUND_CAP is 64).

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Negative controls
   ─────────────────────────────────────────────────────────────────────────
   Each control changes one rule of the model in a scratch copy. The copy
   must fail to compile, and its first error must be in a lemma whose
   statement the change makes false. The rows use the CONTROLS format of
   scripts/verify-reified/rocq-negative-controls.py.
   pq-head-greatest               the lane-head scan keeps the greater
                                  order → below_false_le
   pq-head-by-position            the head of a lane is its first entry
                                  → least_spec
   pq-unprioritized-first         the unprioritized level comes first
                                  → lvl_key_le
   pq-first-match-by-position     a receive binds the first matching head
                                  in list order → first_match_spec
   pq-level-lane-sees-all         a level queue sees every level
                                  → sees_level
   pq-fire-behind-head            a receive binds a record behind a level
                                  head → pq_unrestricted_first_matching_level
   pq-guard-every-head            a guard refusal moves on to the next
                                  level head → pq_unrestricted_first_matching_level
   pq-settle-any-order            the settle fires the first fireable head
                                  in list order → oldest_spec
   pq-offer-any-order             a produce plan ignores the orders of the
                                  candidates → offer_pick_older_first
   pq-offer-refusal-stores        a refusal stores the datum although the
                                  other candidate fits → offer_pick_older_first
   pq-no-settle-after             a completed plan has no settle after it
                                  → with_settles_inv
   pq-no-lift                     the settle does not lift a stored record
                                  → settle_step_records
   pq-land-drops                  the record in flight never lands
                                  → run_land_records
   pq-view-without-flight         the record in flight is not a level head
                                  of the view → run_land_records
   pq-no-land-first               a fire of a stored record does not land
                                  the record in flight first
                                  → next_fire_bound
   pq-peek-spends-lifted          a fire of a peek waiter spends the lifted
                                  record → settle_loop_keeps
   pq-no-cap-check                at_cap is always false
                                  → settle_loop_bounds
   pq-no-divergent-end            a divergent pair goes on
                                  → settle_loop_bounds
   pq-peek-no-restore             a peek of a record stages no re-store
                                  → pq_peek_restores_head
   pq-fresh-jumps-queue           a fresh datum plans behind an older head
                                  of its level → pq_fifo_within_level
   pq-reentry-at-head             a re-entered datum plans whatever the
                                  head is → pq_fifo_within_level
   pq-reentry-empty-level-stores  a re-entry on an empty level is stored
                                  → reentry_at_fifo_place
   pq-reentry-waiter-at-head      a re-entered continuation plans whatever
                                  the head is → pq_fifo_within_level
   pq-reentry-empty-lane-parks    a re-entry on an empty lane parks
                                  → reentry_at_fifo_place
   pq-reissue-fresh-ord           carried is always None
                                  → pq_reissue_takes_level_fifo_place
   pq-no-bump                     the bump keeps the counter value
                                  → pq_reissue_takes_level_fifo_place
   pq-perm-without-distinct-ords  dist holds for every view → apply_equiv
   pq-priority-negative           check_values admits a negative priority
                                  → pq_priority_domain
   pq-priority-bound-ignored      check_values ignores the bound
                                  → pq_priority_domain
   pq-level-storage               a store keeps an entry for its level too
                                  → store_after_bump
   pq-read-before-reserve         a step reads before it reserves
                                  → step_covered
   pq-unprepaid-unwrap            a settle round does not prepay the unwrap
                                  → round_staged_shape
   The controls of revision I-21 (one fire count for each transaction):
   pq-plan-opens-at-cap           plan_at_cap is always false
                                  → pq_settle_terminates
   pq-plan-restarts-count         the plan starts at 0 fires
                                  → pq_settle_terminates
   pq-after-settle-restarts-count the settle after the plan starts at 0
                                  fires → pq_settle_terminates

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   StackDiscipline.v lists the strategies that failed for the stack. They
   apply here too. The strategies below failed for the priority queue.
   - A section named PriorityQueue masks the name of the module ("masking
     absolute name"). The section is Pq.
   - The section variable G (the type of a guard) clashes with a hypothesis
     named G. Name such hypotheses Gk.
   - in_map with a key function under a lambda does not unify. Pass the
     function explicitly: in_map (fun e0 => (k1 e0, k2 e0)).
   - cbn [receive_loop] leaves s_records (bumped st) folded, so a rewrite
     with a fact about s_records st fails. Add bumped to the list of cbn.
   - cbn [offer_loop] does not unfold offer_loop (S cap) when bumped st stays
     folded in its argument. Prove the step as a lemma (offer_loop_store)
     and apply it.
   - A level exists exactly when a record holds it. The direction from the
     head needs no classical logic: destruct record_head and use
     record_head_spec, not a double negation.
   - least (e :: l) is Some e or least l, but an induction on least_from does
     not prove it simply (the best changes state). Use least_is_min with
     distinct keys (first_match_cons_behind) or the specification of
     first_match (store_fresh_level_inv).
   - The level keys of the heads in first_match are distinct only because a
     level has one head. Derive their NoDup from distinct orders with
     nodup_key_of_ords and level_heads_inj, not from NoDup of the records.
   - A proof that unfolds a rule which a control mutates breaks a lemma
     whose statement stays true. Name each such rule (oldest, lands_first,
     cand1, cand2, sees) and destruct the named term without unfolding it.
     Only the lemma that states the rule unfolds it (oldest_spec,
     next_fire_bound, sees_level).
   - The headlines that the brief names (v2 section 3.1) replace the first
     names of this module (pq_levels_ascend, pq_lane_sees, pq_fifo,
     pq_head_blocks, pq_reentry_at_fifo_place, pq_reissue_keeps_ord,
     pq_peek_restore_needs_no_settle, pq_settle_trace_covered). The
     reified_priority_queue.rs suite cites the names of the brief.
   ═══════════════════════════════════════════════════════════════════════ *)
From Stdlib Require Import List Arith Bool Lia PeanoNat ZArith.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── 1. The fire rounds ────────────────────────────────────────────────── *)

Inductive fire_step : Type := FDone | FContinue | FDivergent | FRoundCap.

Inductive reissue : Type := ReissueDivergent | ReissueRoundCap.

(* has o: the option o holds a value. *)
Definition has {A : Type} (o : option A) : bool := match o with Some _ => true | None => false end.

(* ── 2. The least entry of a list by a key of two numbers ─────────────── *)

Section Least.
  Variable E : Type.
  Variable k1 k2 : E -> nat.

  (* below a b: the key (k1, k2) of a is less than the key of b. *)
  Definition below (a b : E) : bool := (k1 a <? k1 b) || ((k1 a =? k1 b) && (k2 a <? k2 b)).

  (* key_le a b: the key of a is at most the key of b. *)
  Definition key_le (a b : E) : Prop := k1 a < k1 b \/ (k1 a = k1 b /\ k2 a <= k2 b).

  (* The scan of a lane head is PriorityQueueDiscipline::lane_heads with
     head_metered on each pair. A later entry replaces the best only when its
     key is less. So on equal keys the earlier entry stays. *)
  Fixpoint least_from (best : option E) (l : list E) : option E :=
    match l with
    | [] => best
    | e :: rest =>
        least_from (match best with
                    | None => Some e
                    | Some b => if below e b then Some e else Some b
                    end) rest
    end.

  Definition least (l : list E) : option E := least_from None l.

  Lemma below_false_le : forall a b, below a b = false -> key_le b a.
  Proof.
    intros a b H. unfold below in H. apply orb_false_iff in H as [H1 H2].
    apply Nat.ltb_ge in H1. unfold key_le.
    destruct (Nat.eq_dec (k1 a) (k1 b)) as [E1 | E1].
    - right. split; [symmetry; exact E1 |].
      rewrite (proj2 (Nat.eqb_eq _ _) E1) in H2. simpl in H2. apply Nat.ltb_ge in H2. exact H2.
    - left. lia.
  Qed.

  Lemma below_true_lt : forall a b, below a b = true -> key_le a b /\ ~ key_le b a.
  Proof.
    intros a b H. unfold below in H. unfold key_le.
    apply orb_true_iff in H as [H | H].
    - apply Nat.ltb_lt in H. split; [left; exact H | lia].
    - apply andb_true_iff in H as [H1 H2]. apply Nat.eqb_eq in H1. apply Nat.ltb_lt in H2.
      split; [right; split; [exact H1 | lia] | lia].
  Qed.

  Lemma key_le_trans : forall a b c, key_le a b -> key_le b c -> key_le a c.
  Proof. unfold key_le. intros a b c H1 H2. lia. Qed.

  Lemma key_le_refl : forall a, key_le a a.
  Proof. intros a. right. split; [reflexivity | lia]. Qed.

  Lemma key_le_total : forall a b, key_le a b \/ key_le b a.
  Proof. unfold key_le. intros a b. lia. Qed.

  (* The step of least_from: the new best has a key at most the key of e and of
     the old best. It is e or the old best. *)
  Lemma least_step :
    forall e best,
      exists b', (match best with None => Some e | Some b => if below e b then Some e else Some b end)
                   = Some b' /\
                 key_le b' e /\ (forall b0, best = Some b0 -> key_le b' b0) /\ (b' = e \/ best = Some b').
  Proof.
    intros e [b0 |].
    - destruct (below e b0) eqn:Eb.
      + exists e. apply below_true_lt in Eb as [Hle _].
        split; [reflexivity | split; [apply key_le_refl | split]].
        * intros b1 Hb1. injection Hb1 as <-. exact Hle.
        * left. reflexivity.
      + exists b0. apply below_false_le in Eb.
        split; [reflexivity | split; [exact Eb | split]].
        * intros b1 Hb1. injection Hb1 as <-. apply key_le_refl.
        * right. reflexivity.
    - exists e. split; [reflexivity | split; [apply key_le_refl | split]].
      + intros b1 Hb1. discriminate.
      + left. reflexivity.
  Qed.

  Lemma least_from_spec :
    forall l best,
      match least_from best l with
      | None => best = None /\ l = []
      | Some t => (best = Some t \/ In t l) /\
                  (forall b, best = Some b -> key_le t b) /\
                  (forall x, In x l -> key_le t x)
      end.
  Proof.
    induction l as [| e rest IH]; intros best; cbn [least_from].
    - destruct best as [b |]; [| split; reflexivity].
      split; [left; reflexivity |]. split; [| intros x []].
      intros b' Hb. injection Hb as <-. apply key_le_refl.
    - destruct (least_step e best) as [b' [Hb' [He [Hbest Horigin]]]]. rewrite Hb'.
      specialize (IH (Some b')).
      destruct (least_from (Some b') rest) as [t |]; [| destruct IH as [Hn _]; discriminate].
      destruct IH as [Ht [Htb Hrest]].
      assert (Htb' : key_le t b') by (apply Htb; reflexivity).
      split; [| split].
      + destruct Ht as [Ht | Ht].
        * injection Ht as <-. destruct Horigin as [-> | Hst]; [right; left; reflexivity | left; exact Hst].
        * right; right; exact Ht.
      + intros b0 Hb0. exact (key_le_trans _ _ _ Htb' (Hbest b0 Hb0)).
      + intros x [Hx | Hx]; [subst x; exact (key_le_trans _ _ _ Htb' He) | exact (Hrest x Hx)].
  Qed.

  Lemma least_spec : forall l t, least l = Some t -> In t l /\ forall x, In x l -> key_le t x.
  Proof.
    intros l t H. unfold least in H. pose proof (least_from_spec l None) as Hs. rewrite H in Hs.
    destruct Hs as [[Hn | Hin] [_ Hall]]; [discriminate | split; [exact Hin | exact Hall]].
  Qed.

  Lemma least_none : forall l, least l = None <-> l = [].
  Proof.
    intros l. unfold least. pose proof (least_from_spec l None) as Hs.
    destruct (least_from None l) as [t |]; split; intros H.
    - discriminate.
    - subst l. destruct Hs as [[Hn | []] _]. discriminate.
    - exact (proj2 Hs).
    - reflexivity.
  Qed.

  Lemma least_some : forall l x, In x l -> exists t, least l = Some t.
  Proof.
    intros l x Hx. destruct (least l) as [t |] eqn:Ht; [exists t; reflexivity |].
    apply least_none in Ht. subst l. destruct Hx.
  Qed.

  (* Distinct keys: no two entries of the list share their key. *)
  Definition keys_distinct (l : list E) : Prop := NoDup (map (fun e => (k1 e, k2 e)) l).

  Lemma keys_distinct_inj :
    forall l a b, keys_distinct l -> In a l -> In b l -> k1 a = k1 b -> k2 a = k2 b -> a = b.
  Proof.
    induction l as [| e rest IH]; intros a b Hnd Ha Hb H1 H2; [destruct Ha |].
    unfold keys_distinct in Hnd. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct Ha as [Ha | Ha]; destruct Hb as [Hb | Hb]; subst.
    - reflexivity.
    - exfalso. apply Hnot. rewrite H1, H2. apply (in_map (fun e => (k1 e, k2 e))). exact Hb.
    - exfalso. apply Hnot. rewrite <- H1, <- H2. apply (in_map (fun e => (k1 e, k2 e))). exact Ha.
    - exact (IH a b Hnd Ha Hb H1 H2).
  Qed.

  (* With distinct keys, an entry whose key is at most every key is the least. *)
  Lemma least_is_min :
    forall l t, keys_distinct l -> In t l -> (forall x, In x l -> key_le t x) -> least l = Some t.
  Proof.
    intros l t Hnd Ht Hmin.
    destruct (least_some l t Ht) as [t' Ht']. rewrite Ht'.
    destruct (least_spec l t' Ht') as [Hin' Hmin'].
    pose proof (Hmin t' Hin') as G1. pose proof (Hmin' t Ht) as G2.
    unfold key_le in G1, G2. f_equal.
    apply (keys_distinct_inj l t' t Hnd Hin' Ht); lia.
  Qed.

  Lemma keys_distinct_perm :
    forall l1 l2, Permutation l1 l2 -> keys_distinct l1 -> keys_distinct l2.
  Proof.
    intros l1 l2 Hp Hnd. unfold keys_distinct in *.
    eapply Permutation_NoDup; [apply Permutation_map; exact Hp | exact Hnd].
  Qed.

  (* Permuted lists with distinct keys have one least entry. *)
  Lemma least_perm :
    forall l1 l2, Permutation l1 l2 -> keys_distinct l1 -> least l1 = least l2.
  Proof.
    intros l1 l2 Hp Hnd. destruct l1 as [| a rest].
    - apply Permutation_nil in Hp. subst l2. reflexivity.
    - destruct (least_some (a :: rest) a (or_introl eq_refl)) as [t Ht]. rewrite Ht.
      destruct (least_spec _ t Ht) as [Hin Hmin].
      symmetry. apply least_is_min.
      + eapply keys_distinct_perm; eassumption.
      + eapply Permutation_in; eassumption.
      + intros x Hx. apply Hmin. eapply Permutation_in; [apply Permutation_sym; exact Hp | exact Hx].
  Qed.

  Lemma keys_distinct_filter :
    forall (f : E -> bool) l, keys_distinct l -> keys_distinct (filter f l).
  Proof.
    intros f l. induction l as [| e rest IH]; intros Hnd; [exact Hnd |].
    unfold keys_distinct in *. cbn [map filter] in *. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct (f e); [| exact (IH Hnd)].
    cbn [map]. apply NoDup_cons; [| exact (IH Hnd)].
    intros Hin. apply Hnot. apply in_map_iff in Hin as [y [Hy Hyin]].
    rewrite <- Hy. apply (in_map (fun e0 => (k1 e0, k2 e0))).
    apply filter_In in Hyin as [Hyin _]. exact Hyin.
  Qed.
End Least.

Arguments below {E} k1 k2 a b.
Arguments key_le {E} k1 k2 a b.
Arguments least_from {E} k1 k2 best l.
Arguments least {E} k1 k2 l.
Arguments keys_distinct {E} k1 k2 l.

Section Pq.
  Variable V : Type.     (* the payload of a datum *)
  Variable P : Type.     (* the pattern of a continuation *)
  Variable G : Type.     (* the guard of a continuation *)
  Variable spatial : P -> V -> bool.   (* Cx::bind: the metered spatial match *)
  Variable admits : G -> V -> bool.    (* Cx::admits: the guard over the match *)
  Variable cap : nat.    (* FIRE_ROUND_CAP *)

  (* FireRounds::after_fire with the count of fires after this one. *)
  Definition after_fire (fired : nat) (spent stays : bool) : fire_step :=
    if spent then FDone
    else if stays then FDivergent
    else if cap <=? fired then FRoundCap
    else FContinue.

  (* ── 3. Entries, levels and lanes ──────────────────────────────────── *)

  (* A record of a priority queue has an order, a rank, a level, a payload and
     a persistence. The rank is the position of its bincode bytes in their
     order. The level is Some p for the level p, and None for the unprioritized
     level. *)
  Record record : Type :=
    mkRecord { r_ord : nat; r_rank : nat; r_lvl : option nat; r_val : V; r_pers : bool }.

  (* A waiter has an order, a rank, a lane, a pattern, a guard, a persistence
     and a peek flag. The lane is Some p for the waiter queue of the level p,
     and None for the unrestricted queue. *)
  Record waiter : Type :=
    mkWaiter { w_ord : nat; w_rank : nat; w_lane : option nat; w_pat : P; w_guard : G;
               w_pers : bool; w_peek : bool }.

  Definition lvl_eqb (a b : option nat) : bool :=
    match a, b with Some x, Some y => x =? y | None, None => true | _, _ => false end.

  Lemma lvl_eqb_iff : forall a b, lvl_eqb a b = true <-> a = b.
  Proof.
    intros [x |] [y |]; cbn; split; intros H; try discriminate; try reflexivity.
    - apply Nat.eqb_eq in H. subst. reflexivity.
    - injection H as ->. apply Nat.eqb_refl.
  Qed.

  Lemma lvl_eqb_refl : forall a, lvl_eqb a a = true.
  Proof. intros a. apply lvl_eqb_iff. reflexivity. Qed.

  (* The key of a level: the priority levels ascend, and the unprioritized
     level comes after every priority level (F6, the derived order of
     Level). *)
  Definition lvl_class (lvl : option nat) : nat := match lvl with Some _ => 0 | None => 1 end.
  Definition lvl_index (lvl : option nat) : nat := match lvl with Some p => p | None => 0 end.

  (* lvl_le a b: the level a comes no later than the level b. *)
  Definition lvl_le (a b : option nat) : bool :=
    match a, b with
    | Some x, Some y => x <=? y
    | Some _, None => true
    | None, Some _ => false
    | None, None => true
    end.

  (* sees lane lvl: a waiter of the lane sees the records of the level
     (Lane::sees). A level lane sees its own level, and the unrestricted lane
     sees every level. *)
  Definition sees (lane lvl : option nat) : bool :=
    match lane with Some p => lvl_eqb lvl (Some p) | None => true end.

  (* A level lane sees exactly its own level. *)
  Lemma sees_level : forall p lvl, sees (Some p) lvl = true <-> lvl = Some p.
  Proof. intros p lvl. unfold sees. apply lvl_eqb_iff. Qed.

  Lemma sees_any : forall lvl, sees None lvl = true.
  Proof. reflexivity. Qed.

  Definition rec_key_eqb (a b : record) : bool := (r_ord a =? r_ord b) && (r_rank a =? r_rank b).
  Definition wtr_key_eqb (a b : waiter) : bool := (w_ord a =? w_ord b) && (w_rank a =? w_rank b).

  (* The head of a level: its record with the least (ord, rank). *)
  Definition record_head (l : list record) (lvl : option nat) : option record :=
    least r_ord r_rank (filter (fun r => lvl_eqb (r_lvl r) lvl) l).

  (* The head of a lane: its waiter with the least (ord, rank). *)
  Definition waiter_head (l : list waiter) (lane : option nat) : option waiter :=
    least w_ord w_rank (filter (fun w => lvl_eqb (w_lane w) lane) l).

  Definition is_record_head (l : list record) (r : record) : bool :=
    match record_head l (r_lvl r) with Some h => rec_key_eqb h r | None => false end.

  Definition is_waiter_head (l : list waiter) (w : waiter) : bool :=
    match waiter_head l (w_lane w) with Some h => wtr_key_eqb h w | None => false end.

  (* The first level head, in ascending level order, that the lane sees and
     whose payload matches the pattern spatially (first_match of
     priority_queue.rs, decision 38). *)
  Definition first_match (l : list record) (lane : option nat) (pat : P) : option record :=
    least (fun r => lvl_class (r_lvl r)) (fun r => lvl_index (r_lvl r))
      (filter (fun r => is_record_head l r && sees lane (r_lvl r) && spatial pat (r_val r)) l).

  (* The record that a waiter-lane head fires: its first match, when its
     guard admits the match (decision 38: the guard is checked once). *)
  Definition fireable (l : list record) (w : waiter) : option record :=
    match first_match l (w_lane w) (w_pat w) with
    | Some h => if admits (w_guard w) (r_val h) then Some h else None
    | None => None
    end.

  (* ── 4. Facts of heads and of the first match ────────────────────────── *)

  Lemma nodup_pair_of_fst :
    forall (A : Type) (f g : A -> nat) l, NoDup (map f l) -> NoDup (map (fun e => (f e, g e)) l).
  Proof.
    intros A f g l. induction l as [| a rest IH]; intros H; [constructor |].
    cbn [map] in *. apply NoDup_cons_iff in H as [Hnot H]. constructor; [| exact (IH H)].
    intros Hin. apply Hnot. apply in_map_iff in Hin as [b [Hb Hbin]]. injection Hb as Hf _.
    rewrite <- Hf. apply in_map. exact Hbin.
  Qed.

  Lemma ords_inj :
    forall (A : Type) (f : A -> nat) l a b, NoDup (map f l) -> In a l -> In b l -> f a = f b -> a = b.
  Proof.
    intros A f l. induction l as [| e rest IH]; intros a b Hnd Ha Hb Hab; [destruct Ha |].
    cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct Ha as [Ha | Ha]; destruct Hb as [Hb | Hb]; subst.
    - reflexivity.
    - exfalso. apply Hnot. rewrite Hab. apply in_map. exact Hb.
    - exfalso. apply Hnot. rewrite <- Hab. apply in_map. exact Ha.
    - exact (IH a b Hnd Ha Hb Hab).
  Qed.

  Lemma record_head_spec :
    forall l lvl h, record_head l lvl = Some h ->
      In h l /\ r_lvl h = lvl /\ forall x, In x l -> r_lvl x = lvl -> key_le r_ord r_rank h x.
  Proof.
    intros l lvl h H. unfold record_head in H.
    destruct (least_spec _ r_ord r_rank _ h H) as [Hin Hmin].
    apply filter_In in Hin as [Hin Hlvl]. apply lvl_eqb_iff in Hlvl.
    split; [exact Hin | split; [exact Hlvl |]].
    intros x Hx Hxl. apply Hmin. apply filter_In. split; [exact Hx | apply lvl_eqb_iff; exact Hxl].
  Qed.

  Lemma record_head_none :
    forall l lvl, record_head l lvl = None <-> forall x, In x l -> r_lvl x <> lvl.
  Proof.
    intros l lvl. unfold record_head. rewrite least_none. split.
    - intros H x Hx Hlvl. assert (Hin : In x (filter (fun r => lvl_eqb (r_lvl r) lvl) l))
        by (apply filter_In; split; [exact Hx | apply lvl_eqb_iff; exact Hlvl]).
      rewrite H in Hin. destruct Hin.
    - intros H. destruct (filter (fun r => lvl_eqb (r_lvl r) lvl) l) as [| y rest] eqn:Ef;
        [reflexivity |].
      exfalso. assert (Hy : In y (filter (fun r => lvl_eqb (r_lvl r) lvl) l))
        by (rewrite Ef; left; reflexivity).
      apply filter_In in Hy as [Hy Hlvl]. apply lvl_eqb_iff in Hlvl. exact (H y Hy Hlvl).
  Qed.

  Lemma waiter_head_spec :
    forall l lane w, waiter_head l lane = Some w ->
      In w l /\ w_lane w = lane /\ forall x, In x l -> w_lane x = lane -> key_le w_ord w_rank w x.
  Proof.
    intros l lane w H. unfold waiter_head in H.
    destruct (least_spec _ w_ord w_rank _ w H) as [Hin Hmin].
    apply filter_In in Hin as [Hin Hlane]. apply lvl_eqb_iff in Hlane.
    split; [exact Hin | split; [exact Hlane |]].
    intros x Hx Hxl. apply Hmin. apply filter_In. split; [exact Hx | apply lvl_eqb_iff; exact Hxl].
  Qed.

  Lemma waiter_head_none :
    forall l lane, waiter_head l lane = None <-> forall x, In x l -> w_lane x <> lane.
  Proof.
    intros l lane. unfold waiter_head. rewrite least_none. split.
    - intros H x Hx Hlane. assert (Hin : In x (filter (fun w => lvl_eqb (w_lane w) lane) l))
        by (apply filter_In; split; [exact Hx | apply lvl_eqb_iff; exact Hlane]).
      rewrite H in Hin. destruct Hin.
    - intros H. destruct (filter (fun w => lvl_eqb (w_lane w) lane) l) as [| y rest] eqn:Ef;
        [reflexivity |].
      exfalso. assert (Hy : In y (filter (fun w => lvl_eqb (w_lane w) lane) l))
        by (rewrite Ef; left; reflexivity).
      apply filter_In in Hy as [Hy Hlane]. apply lvl_eqb_iff in Hlane. exact (H y Hy Hlane).
  Qed.

  (* With distinct orders, a head is the entry itself. *)
  Lemma is_record_head_iff :
    forall l r, NoDup (map r_ord l) -> In r l ->
      is_record_head l r = true <-> record_head l (r_lvl r) = Some r.
  Proof.
    intros l r Hnd Hr. unfold is_record_head. split.
    - destruct (record_head l (r_lvl r)) as [h |] eqn:Eh; [| discriminate].
      intros Hk. unfold rec_key_eqb in Hk. apply andb_true_iff in Hk as [Ho _].
      apply Nat.eqb_eq in Ho. destruct (record_head_spec l (r_lvl r) h Eh) as [Hin _].
      f_equal. exact (ords_inj record r_ord l h r Hnd Hin Hr Ho).
    - intros ->. unfold rec_key_eqb. rewrite !Nat.eqb_refl. reflexivity.
  Qed.

  Lemma is_waiter_head_iff :
    forall l w, NoDup (map w_ord l) -> In w l ->
      is_waiter_head l w = true <-> waiter_head l (w_lane w) = Some w.
  Proof.
    intros l w Hnd Hw. unfold is_waiter_head. split.
    - destruct (waiter_head l (w_lane w)) as [h |] eqn:Eh; [| discriminate].
      intros Hk. unfold wtr_key_eqb in Hk. apply andb_true_iff in Hk as [Ho _].
      apply Nat.eqb_eq in Ho. destruct (waiter_head_spec l (w_lane w) h Eh) as [Hin _].
      f_equal. exact (ords_inj waiter w_ord l h w Hnd Hin Hw Ho).
    - intros ->. unfold wtr_key_eqb. rewrite !Nat.eqb_refl. reflexivity.
  Qed.

  (* The key order of levels is lvl_le. *)
  Lemma lvl_key_le :
    forall a b : record,
      key_le (fun r => lvl_class (r_lvl r)) (fun r => lvl_index (r_lvl r)) a b <->
      lvl_le (r_lvl a) (r_lvl b) = true.
  Proof.
    intros a b. unfold key_le. destruct (r_lvl a) as [x |], (r_lvl b) as [y |]; cbn; split;
      intros H; try lia; try reflexivity; try discriminate.
    - apply Nat.leb_le. lia.
    - apply Nat.leb_le in H. lia.
  Qed.

  Lemma first_match_spec :
    forall l lane pat h, first_match l lane pat = Some h ->
      In h l /\ is_record_head l h = true /\ sees lane (r_lvl h) = true /\
      spatial pat (r_val h) = true /\
      (forall x, In x l -> is_record_head l x = true -> sees lane (r_lvl x) = true ->
                 spatial pat (r_val x) = true -> lvl_le (r_lvl h) (r_lvl x) = true).
  Proof.
    intros l lane pat h H. unfold first_match in H.
    destruct (least_spec _ _ _ _ h H) as [Hin Hmin].
    apply filter_In in Hin as [Hin Hc]. apply andb_true_iff in Hc as [Hc Hsp].
    apply andb_true_iff in Hc as [Hhead Hsees].
    split; [exact Hin | split; [exact Hhead | split; [exact Hsees | split; [exact Hsp |]]]].
    intros x Hx Hxh Hxs Hxsp. apply lvl_key_le. apply Hmin. apply filter_In.
    split; [exact Hx | rewrite Hxh, Hxs, Hxsp; reflexivity].
  Qed.

  Lemma first_match_none :
    forall l lane pat, first_match l lane pat = None ->
      forall x, In x l -> is_record_head l x = true -> sees lane (r_lvl x) = true ->
                spatial pat (r_val x) = false.
  Proof.
    intros l lane pat H x Hx Hxh Hxs. unfold first_match in H. apply least_none in H.
    destruct (spatial pat (r_val x)) eqn:Esp; [| reflexivity]. exfalso.
    assert (Hin : In x (filter (fun r => is_record_head l r && sees lane (r_lvl r) &&
                                        spatial pat (r_val r)) l))
      by (apply filter_In; split; [exact Hx | rewrite Hxh, Hxs, Esp; reflexivity]).
    rewrite H in Hin. destruct Hin.
  Qed.

  Lemma fireable_spec :
    forall l w h, fireable l w = Some h ->
      first_match l (w_lane w) (w_pat w) = Some h /\ admits (w_guard w) (r_val h) = true.
  Proof.
    intros l w h H. unfold fireable in H.
    destruct (first_match l (w_lane w) (w_pat w)) as [h0 |]; [| discriminate].
    destruct (admits (w_guard w) (r_val h0)) eqn:Ea; [| discriminate].
    injection H as <-. split; [reflexivity | exact Ea].
  Qed.

  (* The order of the levels (lvl_le) is a total order. *)
  Lemma lvl_le_total_order :
    (forall a b, lvl_le a b = true -> lvl_le b a = true -> a = b) /\
    (forall a b c, lvl_le a b = true -> lvl_le b c = true -> lvl_le a c = true) /\
    (forall a b, lvl_le a b = true \/ lvl_le b a = true).
  Proof.
    split; [| split].
    - intros [x |] [y |]; cbn; intros H1 H2; try discriminate; try reflexivity.
      apply Nat.leb_le in H1, H2. f_equal. lia.
    - intros [x |] [y |] [z |]; cbn; intros H1 H2; try discriminate; try reflexivity.
      apply Nat.leb_le in H1, H2. apply Nat.leb_le. lia.
    - intros [x |] [y |]; cbn; try (left; reflexivity); try (right; reflexivity).
      destruct (Nat.le_ge_cases x y) as [H | H];
        [left; apply Nat.leb_le; exact H | right; apply Nat.leb_le; exact H].
  Qed.

  (* Headline 1. The levels ascend (PqLevelOrder, F6, decision 38). The
     levels form a total order in which the priority levels ascend. A
     receive binds the first level head, in this order, that its lane sees
     and whose payload matches its pattern. When no such head exists, every
     level head that the lane sees fails the spatial match, whatever the
     records behind the heads are. *)
  Theorem pq_level_order :
    (forall p q, lvl_le (Some p) (Some q) = (p <=? q)) /\
    (forall a b, lvl_le a b = true -> lvl_le b a = true -> a = b) /\
    (forall a b c, lvl_le a b = true -> lvl_le b c = true -> lvl_le a c = true) /\
    (forall a b, lvl_le a b = true \/ lvl_le b a = true) /\
    (forall l lane pat h, first_match l lane pat = Some h ->
       In h l /\ is_record_head l h = true /\ sees lane (r_lvl h) = true /\
       spatial pat (r_val h) = true /\
       (forall x, In x l -> is_record_head l x = true -> sees lane (r_lvl x) = true ->
                  spatial pat (r_val x) = true -> lvl_le (r_lvl h) (r_lvl x) = true)) /\
    (forall l lane pat, first_match l lane pat = None ->
       forall x, In x l -> is_record_head l x = true -> sees lane (r_lvl x) = true ->
                 spatial pat (r_val x) = false).
  Proof.
    destruct lvl_le_total_order as [Ha [Ht Hto]].
    split; [reflexivity |].
    split; [exact Ha | split; [exact Ht | split; [exact Hto |]]].
    split; [exact first_match_spec | exact first_match_none].
  Qed.

  (* Headline 2. The unprioritized level is the lowest level (F6, the port of
     the default level of the legacy priority queue). It comes after every
     priority level. So a receive binds an unprioritized record only when no
     head of a priority level that its lane sees matches its pattern. *)
  Theorem pq_unprioritized_lowest :
    (forall p, lvl_le (Some p) None = true /\ lvl_le None (Some p) = false) /\
    (forall l lane pat h, first_match l lane pat = Some h -> r_lvl h = None ->
       forall x, In x l -> is_record_head l x = true -> sees lane (r_lvl x) = true ->
                 r_lvl x <> None -> spatial pat (r_val x) = false).
  Proof.
    split; [intros p; split; reflexivity |].
    intros l lane pat h Hm Hh x Hx Hxh Hxs Hxl.
    destruct (spatial pat (r_val x)) eqn:Esp; [| reflexivity]. exfalso.
    destruct (first_match_spec l lane pat h Hm) as [_ [_ [_ [_ Hmin]]]].
    pose proof (Hmin x Hx Hxh Hxs Esp) as Hle. rewrite Hh in Hle.
    destruct (r_lvl x) as [q |]; [discriminate | exact (Hxl eq_refl)].
  Qed.

  (* ── 5. States, staged operations and their effects ───────────────────── *)

  (* A collection: its records, its waiting continuations, and its counter
     (None before the first operation). *)
  Record sstate : Type :=
    mkS { s_records : list record; s_waiters : list waiter; s_counter : option nat }.

  (* The bump of spaces/counter.rs: the ord is the counter value, or 0 for
     the first operation, which opens the counter at 1. One counter serves the
     whole priority queue. *)
  Definition bump_ord (c : option nat) : nat := match c with None => 0 | Some v => v end.
  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).
  Definition bumped (st : sstate) : sstate :=
    mkS (s_records st) (s_waiters st) (bump_next (s_counter st)).

  Inductive staged : Type :=
  | SStore : record -> staged                 (* the store of the datum of the operation *)
  | SPark : waiter -> staged                  (* the park of its continuation *)
  | SFireProduce : waiter -> record -> staged (* a produce-side fire: the record is incoming *)
  | SFireConsume : waiter -> record -> staged (* a consume-side fire: the waiter is incoming *)
  | SLift : record -> staged                  (* K_lift removes a stored record *)
  | SRestore : record -> staged.              (* Rule E: the record is stored again *)

  Fixpoint drop_key {E : Type} (ord rank : E -> nat) (o k : nat) (l : list E) : list E :=
    match l with
    | [] => []
    | x :: rest =>
        if (ord x =? o) && (rank x =? k) then rest else x :: drop_key ord rank o k rest
    end.

  Definition drop_record (h : record) (l : list record) : list record :=
    drop_key r_ord r_rank (r_ord h) (r_rank h) l.
  Definition drop_waiter (w : waiter) (l : list waiter) : list waiter :=
    drop_key w_ord w_rank (w_ord w) (w_rank w) l.

  (* The effect of a staged operation on the staged view. A store puts its
     record in front. A fire removes the fired waiter unless it is persistent.
     A produce-side fire never stores its incoming record. A consume-side fire
     removes the record that it binds unless it is persistent, also for a peek
     (Rule E). The code then stores the record again (SRestore). *)
  Definition apply (st : sstate) (op : staged) : sstate :=
    match op with
    | SStore r => mkS (r :: s_records st) (s_waiters st) (s_counter st)
    | SRestore r => mkS (r :: s_records st) (s_waiters st) (s_counter st)
    | SPark w => mkS (s_records st) (w :: s_waiters st) (s_counter st)
    | SFireProduce w _ =>
        if w_pers w then st else mkS (s_records st) (drop_waiter w (s_waiters st)) (s_counter st)
    | SFireConsume _ h =>
        if r_pers h then st else mkS (drop_record h (s_records st)) (s_waiters st) (s_counter st)
    | SLift h => mkS (drop_record h (s_records st)) (s_waiters st) (s_counter st)
    end.

  Definition run (st : sstate) (ops : list staged) : sstate := fold_left apply ops st.

  Lemma run_app : forall st a b, run st (a ++ b) = run (run st a) b.
  Proof. intros st a b. unfold run. apply fold_left_app. Qed.

  Lemma run_cons : forall st op rest, run st (op :: rest) = run (apply st op) rest.
  Proof. reflexivity. Qed.

  Lemma run_nil : forall st, run st [] = st.
  Proof. reflexivity. Qed.

  Definition is_fire (op : staged) : bool :=
    match op with SFireProduce _ _ | SFireConsume _ _ => true | _ => false end.
  Definition fires (ops : list staged) : nat := length (filter is_fire ops).

  Lemma fires_app : forall a b, fires (a ++ b) = fires a + fires b.
  Proof. intros a b. unfold fires. rewrite filter_app, length_app. reflexivity. Qed.

  Lemma fires_cons :
    forall op rest, fires (op :: rest) = (if is_fire op then 1 else 0) + fires rest.
  Proof. intros op rest. unfold fires. cbn [filter]. destruct (is_fire op); reflexivity. Qed.

  (* ── 6. The settle (settle.rs with PriorityQueueView) ───────────────────── *)

  Inductive settle_end : Type := Quiescent | SettleReissue (c : reissue).

  Record settled : Type := mkSettled { z_ops : list staged; z_end : settle_end }.

  (* The landing of the record in flight: it is stored again at its order and
     level, with its own bytes, so with its identity. *)
  Definition land (x : option record) : list staged :=
    match x with None => [] | Some r => [SRestore r] end.

  (* The records of the view of a settle: the stored records and the record
     in flight (record_head_list with flight). *)
  Definition view (st : sstate) (x : option record) : list record :=
    match x with Some r => r :: s_records st | None => s_records st end.

  (* The fireable waiter-lane heads of the view. *)
  Definition fire_candidates (st : sstate) (x : option record) : list waiter :=
    filter (fun w => is_waiter_head (s_waiters st) w && has (fireable (view st x) w))
      (s_waiters st).

  (* The settle order of PriorityQueueView (readiness review S4): the waiter
     with the least (ord, rank), so the oldest waiter first. *)
  Definition oldest (l : list waiter) : option waiter := least w_ord w_rank l.

  Lemma oldest_spec :
    forall l w, oldest l = Some w -> In w l /\ forall u, In u l -> key_le w_ord w_rank w u.
  Proof. intros l w H. exact (least_spec _ w_ord w_rank l w H). Qed.

  Lemma oldest_none : forall l, oldest l = None -> l = [].
  Proof. intros l H. unfold oldest in H. apply least_none in H. exact H. Qed.

  Lemma oldest_some : forall l u, In u l -> exists t, oldest l = Some t.
  Proof. intros l u H. exact (least_some _ w_ord w_rank l u H). Qed.

  (* PriorityQueueView::next_fire: the oldest fireable waiter-lane head, the
     record that it fires, and true when that record is the record in flight
     (BoundRecord::InFlight). *)
  Definition next_fire (st : sstate) (x : option record) : option (waiter * record * bool) :=
    match oldest (fire_candidates st x) with
    | Some w =>
        match fireable (view st x) w with
        | Some r => Some (w, r, match x with Some r0 => rec_key_eqb r r0 | None => false end)
        | None => None
        end
    | None => None
    end.

  (* The lift of a fire of the settle: a stored record that is not
     persistent leaves the store before its re-offer. A record in flight is
     already out of the store. *)
  Definition lift_ops (x : option record) (r : record) : list staged :=
    match x with Some _ => [] | None => if r_pers r then [] else [SLift r] end.

  Definition flying (x : option record) (r : record) : bool :=
    match x with Some _ => true | None => negb (r_pers r) end.

  Definition flight_after (x : option record) (r : record) (spent : bool) : option record :=
    if flying x r && negb spent then Some r else None.

  (* The cap check at the top of a settle round (I-21). *)
  Definition at_cap (fired : nat) : bool := cap <=? fired.

  (* The land-first rule of settle.rs: a fire binds a stored record while a
     record is in flight. The record in flight has no fire, so it lands first,
     and the view plans again with the record in its place. *)
  Definition lands_first (x : option record) (in_flight : bool) : bool := has x && negb in_flight.

  (* settle: the loop of settle.rs. *)
  Fixpoint settle_loop (fuel fired : nat) (st : sstate) (x : option record) : settled :=
    match fuel with
    | 0 => mkSettled (land x) (SettleReissue ReissueRoundCap)
    | S fuel' =>
        match next_fire st x with
        | None => mkSettled (land x) Quiescent
        | Some (w, r, in_flight) =>
            if lands_first x in_flight then
              let rest := settle_loop fuel' fired (run st (land x)) None in
              mkSettled (land x ++ z_ops rest) (z_end rest)
            else if at_cap fired then mkSettled (land x) (SettleReissue ReissueRoundCap)
            else
              let ops := lift_ops x r ++ [SFireProduce w r] in
              let spent := negb (r_pers r) && negb (w_peek w) in
              let x1 := flight_after x r spent in
              match after_fire (S fired) spent (w_pers w) with
              | FDone | FContinue =>
                  let rest := settle_loop fuel' (S fired) (run st ops) x1 in
                  mkSettled (ops ++ z_ops rest) (z_end rest)
              | FDivergent => mkSettled (ops ++ land x1) (SettleReissue ReissueDivergent)
              | FRoundCap => mkSettled (ops ++ land x1) (SettleReissue ReissueRoundCap)
              end
        end
    end.

  (* Each fire takes one round of the loop, and each landing takes one more. So
     2 * cap + 1 rounds are enough for at most cap fires. *)
  Definition settle_fuel : nat := S (2 * cap).

  (* A settle call that starts its transaction: the settle before the plan
     and the settle of a settle job. *)
  Definition settle_of (st : sstate) : settled := settle_loop settle_fuel 0 st None.

  (* The settle after the plan (I-21): it continues the fire count. *)
  Definition settle_at (fired : nat) (st : sstate) : settled := settle_loop settle_fuel fired st None.

  (* ── 7. The plans of an operation (PQ-OFFER and PQ-RECEIVE) ─────────── *)

  Inductive plan_end : Type := PDone | PReissue (c : reissue).

  Record planned : Type := mkPlanned { p_ops : list staged; p_end : plan_end }.

  (* Cx::matches: the spatial match, then the guard. *)
  Definition fits_datum (w : waiter) (v : V) : bool := spatial (w_pat w) v && admits (w_guard w) v.

  (* The candidate heads of the datum r (Level::candidate_lanes). For a
     priority level, the first candidate is the head of the waiter queue of its
     level. The second candidate is the head of the unrestricted queue. A
     refused lane has no candidate. *)
  Definition cand1 (ws : list waiter) (r : record) (ref1 : bool) : option waiter :=
    if ref1 then None else match r_lvl r with Some p => waiter_head ws (Some p) | None => None end.

  Definition cand2 (ws : list waiter) (ref2 : bool) : option waiter :=
    if ref2 then None else waiter_head ws None.

  (* offer_target: the older candidate head is tested first (decision 38).
     The first head that fits fires, and each head that refuses refuses its
     lane (decision 37). *)
  Definition offer_pick (ws : list waiter) (r : record) (refused : bool * bool)
    : option waiter * (bool * bool) :=
    let '(ref1, ref2) := refused in
    let v := r_val r in
    match cand1 ws r ref1, cand2 ws ref2 with
    | Some w1, Some w2 =>
        if w_ord w2 <? w_ord w1 then
          if fits_datum w2 v then (Some w2, (ref1, ref2))
          else if fits_datum w1 v then (Some w1, (ref1, true))
          else (None, (true, true))
        else
          if fits_datum w1 v then (Some w1, (ref1, ref2))
          else if fits_datum w2 v then (Some w2, (true, ref2))
          else (None, (true, true))
    | Some w1, None => if fits_datum w1 v then (Some w1, (ref1, ref2)) else (None, (true, ref2))
    | None, Some w2 => if fits_datum w2 v then (Some w2, (ref1, ref2)) else (None, (ref1, true))
    | None, None => (None, (ref1, ref2))
    end.

  (* The loop of offer_pq for the datum r at the head of its level. *)
  Fixpoint offer_loop (fuel fired : nat) (st : sstate) (r : record) (refused : bool * bool)
    : planned :=
    match fuel with
    | 0 => mkPlanned [] (PReissue ReissueRoundCap)
    | S fuel' =>
        match offer_pick (s_waiters st) r refused with
        | (None, _) => mkPlanned [SStore r] PDone
        | (Some w, refused') =>
            let spent := negb (r_pers r) && negb (w_peek w) in
            match after_fire (S fired) spent (w_pers w) with
            | FDone => mkPlanned [SFireProduce w r] PDone
            | FDivergent => mkPlanned [SFireProduce w r] (PReissue ReissueDivergent)
            | FRoundCap => mkPlanned [SFireProduce w r] (PReissue ReissueRoundCap)
            | FContinue =>
                let rest := offer_loop fuel' (S fired) (apply st (SFireProduce w r)) r refused' in
                mkPlanned (SFireProduce w r :: p_ops rest) (p_end rest)
            end
        end
    end.

  (* A consume-side fire, and the re-store of decision 45 (Rule E) after a
     peek of a record that is not persistent. *)
  Definition consume_fire (k : waiter) (h : record) : list staged :=
    if w_peek k && negb (r_pers h) then [SFireConsume k h; SRestore h] else [SFireConsume k h].

  (* The loop of receive_pq for the continuation k at the head of its lane. It
     takes the first level head that its lane sees and that its pattern
     matches. Then it checks the guard once, and a refusal parks (decision 38,
     "Park"). *)
  Fixpoint receive_loop (fuel fired : nat) (st : sstate) (k : waiter) : planned :=
    match fuel with
    | 0 => mkPlanned [] (PReissue ReissueRoundCap)
    | S fuel' =>
        match first_match (s_records st) (w_lane k) (w_pat k) with
        | None => mkPlanned [SPark k] PDone
        | Some h =>
            if admits (w_guard k) (r_val h) then
              let fire := consume_fire k h in
              match after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k) with
              | FDone => mkPlanned fire PDone
              | FDivergent => mkPlanned fire (PReissue ReissueDivergent)
              | FRoundCap => mkPlanned fire (PReissue ReissueRoundCap)
              | FContinue =>
                  let rest := receive_loop fuel' (S fired) (run st fire) k in
                  mkPlanned (fire ++ p_ops rest) (p_end rest)
              end
            else mkPlanned [SPark k] PDone
        end
    end.

  (* ── 8. Operations, re-entries and settle jobs ───────────────────────── *)

  Inductive next_round : Type :=
  | NextOperation (c : reissue) (carried : option nat)
  | NextSettle (c : reissue).

  Record outcome : Type := mkOutcome { o_ops : list staged; o_next : option next_round }.

  Definition carried (pers : bool) (o : nat) : option nat := if pers then Some o else None.

  Definition settle_next (e : settle_end) : option next_round :=
    match e with Quiescent => None | SettleReissue c => Some (NextSettle c) end.

  Lemma settle_next_none : forall e, settle_next e = None -> e = Quiescent.
  Proof. intros [| c] H; [reflexivity | discriminate]. Qed.

  Lemma settle_next_operation : forall e c ca, settle_next e <> Some (NextOperation c ca).
  Proof. intros [| c0] c ca H; discriminate. Qed.

  (* The plan-open check of open_plan (I-21). *)
  Definition plan_at_cap (fired : nat) : bool := cap <=? fired.

  Lemma plan_at_cap_false : forall fired, fired < cap -> plan_at_cap fired = false.
  Proof.
    intros fired H. unfold plan_at_cap.
    destruct (cap <=? fired) eqn:E; [apply Nat.leb_le in E; exfalso; lia | reflexivity].
  Qed.

  Lemma plan_at_cap_true : forall fired, plan_at_cap fired = true -> cap <= fired.
  Proof.
    intros fired H. destruct (Nat.le_gt_cases cap fired) as [Hle | Hgt]; [exact Hle |].
    rewrite (plan_at_cap_false fired Hgt) in H. discriminate.
  Qed.

  (* offer_pq and receive_pq around their loops: open_plan, the plan and the
     settle after the plan, with one fire count for the transaction (I-21). *)
  Definition with_settles (st : sstate) (pers : bool) (o : nat) (plan : nat -> sstate -> planned)
    : outcome :=
    let before := settle_of st in
    match z_end before with
    | SettleReissue c => mkOutcome (z_ops before) (Some (NextOperation c (carried pers o)))
    | Quiescent =>
        let f := fires (z_ops before) in
        if plan_at_cap f
        then mkOutcome (z_ops before) (Some (NextOperation ReissueRoundCap (carried pers o)))
        else
          let p := plan f (run st (z_ops before)) in
          match p_end p with
          | PReissue c =>
              mkOutcome (z_ops before ++ p_ops p) (Some (NextOperation c (carried pers o)))
          | PDone =>
              let after := settle_at (f + fires (p_ops p)) (run st (z_ops before ++ p_ops p)) in
              mkOutcome (z_ops before ++ p_ops p ++ z_ops after) (settle_next (z_end after))
          end
    end.

  (* A fresh datum joins the tail of its level. It is at the head only of an
     empty level (reaches_head with the length of the lane). *)
  Definition fresh_offer (x : record) (f : nat) (s : sstate) : planned :=
    match record_head (s_records s) (r_lvl x) with
    | Some _ => mkPlanned [SStore x] PDone
    | None => offer_loop (S cap) f s x (false, false)
    end.

  (* A fresh continuation joins the tail of its lane. *)
  Definition fresh_receive (k : waiter) (f : nat) (s : sstate) : planned :=
    match waiter_head (s_waiters s) (w_lane k) with
    | Some _ => mkPlanned [SPark k] PDone
    | None => receive_loop (S cap) f s k
    end.

  (* run_with of generic.rs: the bump, then the plans with the ord of the bump. *)
  Definition produce (st : sstate) (rank : nat) (v : V) (lvl : option nat) (pers : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    with_settles (bumped st) pers o (fresh_offer (mkRecord o rank lvl v pers)).

  Definition consume (st : sstate) (rank : nat) (pat : P) (g : G) (lane : option nat)
    (pers peek : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    with_settles (bumped st) pers o (fresh_receive (mkWaiter o rank lane pat g pers peek)).

  (* A settle job (SpaceOp::Settle): the bump, then one settle from the count 0. *)
  Definition settle_job (st : sstate) : outcome :=
    let z := settle_of (bumped st) in
    mkOutcome (z_ops z) (settle_next (z_end z)).

  (* The re-entry of a re-issued persistent element with its first-round order
     (decision 43). The element is at the head of its lane when its lane is
     empty or its order precedes the order of the head (precedes is <).
     Otherwise it is stored or parked at its FIFO place, with no fire. *)
  Definition reenter_offer (x : record) (f : nat) (s : sstate) : planned :=
    match record_head (s_records s) (r_lvl x) with
    | Some h => if r_ord x <? r_ord h then offer_loop (S cap) f s x (false, false)
                else mkPlanned [SStore x] PDone
    | None => offer_loop (S cap) f s x (false, false)
    end.

  Definition reenter_receive (k : waiter) (f : nat) (s : sstate) : planned :=
    match waiter_head (s_waiters s) (w_lane k) with
    | Some w => if w_ord k <? w_ord w then receive_loop (S cap) f s k
                else mkPlanned [SPark k] PDone
    | None => receive_loop (S cap) f s k
    end.

  Definition reenter_produce (st : sstate) (x : record) : outcome :=
    with_settles (bumped st) (r_pers x) (r_ord x) (reenter_offer x).

  Definition reenter_consume (st : sstate) (k : waiter) : outcome :=
    with_settles (bumped st) (w_pers k) (w_ord k) (reenter_receive k).

  (* The staged view after an operation on st. *)
  Definition after (st : sstate) (o : outcome) : sstate := run (bumped st) (o_ops o).

  (* ── 9. INV, and the settle that restores it ─────────────────────────── *)

  (* INV (PqNoPendingMatch): no head of a waiter lane can fire on the level
     heads that it sees. *)
  Definition no_pending_match (st : sstate) : Prop :=
    forall w, In w (s_waiters st) -> is_waiter_head (s_waiters st) w = true ->
      fireable (s_records st) w = None.

  Lemma next_fire_none_iff :
    forall st x, next_fire st x = None <->
      forall w, In w (s_waiters st) -> is_waiter_head (s_waiters st) w = true ->
        fireable (view st x) w = None.
  Proof.
    intros st x. unfold next_fire. split.
    - intros H w Hw Hhead.
      destruct (fireable (view st x) w) as [r |] eqn:Ef; [| reflexivity]. exfalso.
      assert (Hc : In w (fire_candidates st x))
        by (unfold fire_candidates; apply filter_In; split; [exact Hw | rewrite Hhead, Ef; reflexivity]).
      destruct (oldest_some _ w Hc) as [t Ht]. rewrite Ht in H.
      destruct (oldest_spec _ t Ht) as [Htin _].
      unfold fire_candidates in Htin. apply filter_In in Htin as [_ Htc].
      apply andb_true_iff in Htc as [_ Hhas].
      destruct (fireable (view st x) t); [discriminate | discriminate].
    - intros H. destruct (oldest (fire_candidates st x)) as [t |] eqn:Ht;
        [| reflexivity].
      destruct (oldest_spec _ t Ht) as [Htin _].
      unfold fire_candidates in Htin. apply filter_In in Htin as [Htw Htc].
      apply andb_true_iff in Htc as [Hhead Hhas].
      rewrite (H t Htw Hhead) in Hhas. discriminate.
  Qed.

  Lemma next_fire_some :
    forall st x w r b, next_fire st x = Some (w, r, b) ->
      In w (s_waiters st) /\ is_waiter_head (s_waiters st) w = true /\
      fireable (view st x) w = Some r /\
      (forall u, In u (s_waiters st) -> is_waiter_head (s_waiters st) u = true ->
                 fireable (view st x) u <> None -> key_le w_ord w_rank w u) /\
      b = match x with Some r0 => rec_key_eqb r r0 | None => false end.
  Proof.
    intros st x w r b H. unfold next_fire in H.
    destruct (oldest (fire_candidates st x)) as [t |] eqn:Ht; [| discriminate].
    destruct (fireable (view st x) t) as [r' |] eqn:Ef; [| discriminate].
    injection H as <- <- <-.
    destruct (oldest_spec _ t Ht) as [Htin Hmin].
    unfold fire_candidates in Htin. apply filter_In in Htin as [Htw Htc].
    apply andb_true_iff in Htc as [Hhead _].
    split; [exact Htw | split; [exact Hhead | split; [exact Ef | split; [| reflexivity]]]].
    intros u Hu Hhu Hfu. apply Hmin. unfold fire_candidates. apply filter_In.
    split; [exact Hu |]. rewrite Hhu. destruct (fireable (view st x) u); [reflexivity |].
    exfalso. apply Hfu. reflexivity.
  Qed.

  Lemma run_land_records : forall st x, s_records (run st (land x)) = view st x.
  Proof. intros st [r |]; reflexivity. Qed.

  Lemma run_land_waiters : forall st x, s_waiters (run st (land x)) = s_waiters st.
  Proof. intros st [r |]; reflexivity. Qed.

  (* No fire in the view: after the landing of the record in flight, INV. *)
  Lemma next_fire_none_land_inv :
    forall st x, next_fire st x = None -> no_pending_match (run st (land x)).
  Proof.
    intros st x H w Hw Hhead. rewrite run_land_waiters in Hw, Hhead.
    rewrite run_land_records. exact (proj1 (next_fire_none_iff st x) H w Hw Hhead).
  Qed.

  (* A settle that ends Quiescent leaves INV. The view of the settle holds the
     record in flight, so no premise on the record in flight is needed. *)
  Lemma settle_quiescent_inv :
    forall fuel fired st x,
      z_end (settle_loop fuel fired st x) = Quiescent ->
      no_pending_match (run st (z_ops (settle_loop fuel fired st x))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x Hend; cbn [settle_loop] in *.
    - discriminate.
    - destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf.
      + destruct (lands_first x b).
        * cbn [z_ops z_end] in *. rewrite run_app. apply IH. exact Hend.
        * destruct (at_cap fired); [discriminate |].
          destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
            cbn [z_end z_ops] in *; try discriminate; rewrite run_app; apply IH; exact Hend.
      + cbn [z_ops]. apply next_fire_none_land_inv. exact Hnf.
  Qed.

  (* An operation that completes in its round leaves INV. *)
  Lemma with_settles_inv :
    forall st pers o plan,
      o_next (with_settles st pers o plan) = None ->
      no_pending_match (run st (o_ops (with_settles st pers o plan))).
  Proof.
    intros st pers o plan H. unfold with_settles in *.
    destruct (z_end (settle_of st)) as [| c] eqn:Eb; [| discriminate].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); [discriminate |].
    match goal with |- context [p_end ?p] => destruct (p_end p) as [| c] eqn:Ep end;
      [| discriminate].
    cbn [o_ops o_next] in *. rewrite app_assoc, run_app.
    apply settle_next_none in H.
    apply settle_quiescent_inv. exact H.
  Qed.

  (* Headline 8. A produce that completes in its round leaves INV
     (PqNoPendingMatch): no head of a waiter lane can fire on the level heads
     that it sees. The settle after the plan restores INV, so the state before
     the produce needs no premise. This holds for a fresh send and for a
     re-entered send (decision 43). *)
  Theorem pq_produce_preserves_inv :
    (forall st rank v lvl pers,
       o_next (produce st rank v lvl pers) = None ->
       no_pending_match (after st (produce st rank v lvl pers))) /\
    (forall st x, o_next (reenter_produce st x) = None ->
       no_pending_match (after st (reenter_produce st x))).
  Proof.
    split.
    - intros st rank v lvl pers H. unfold after, produce in *. apply with_settles_inv. exact H.
    - intros st x H. unfold after, reenter_produce in *. apply with_settles_inv. exact H.
  Qed.

  (* Headline 9. A consume that completes in its round leaves INV, for a
     fresh receive and for a re-entered receive. *)
  Theorem pq_consume_preserves_inv :
    (forall st rank pat g lane pers peek,
       o_next (consume st rank pat g lane pers peek) = None ->
       no_pending_match (after st (consume st rank pat g lane pers peek))) /\
    (forall st k, o_next (reenter_consume st k) = None ->
       no_pending_match (after st (reenter_consume st k))).
  Proof.
    split.
    - intros st rank pat g lane pers peek H. unfold after, consume in *. apply with_settles_inv.
      exact H.
    - intros st k H. unfold after, reenter_consume in *. apply with_settles_inv. exact H.
  Qed.

  (* ── 10. The settle step ─────────────────────────────────────────────── *)

  (* The orders of a list are distinct. *)
  Definition ords_distinct {E : Type} (ord : E -> nat) (l : list E) : Prop := NoDup (map ord l).

  Lemma drop_key_incl :
    forall (E : Type) (ord rank : E -> nat) o k l x, In x (drop_key ord rank o k l) -> In x l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; intros x Hx; [destruct Hx |].
    cbn [drop_key] in Hx. destruct ((ord y =? o) && (rank y =? k)).
    - right. exact Hx.
    - destruct Hx as [Hx | Hx]; [left; exact Hx | right; apply IH; exact Hx].
  Qed.

  Lemma drop_key_distinct :
    forall (E : Type) (ord rank : E -> nat) l o k,
      ords_distinct ord l -> ords_distinct ord (drop_key ord rank o k l).
  Proof.
    intros E ord rank l o k. induction l as [| y rest IH]; intros Hnd; [exact Hnd |].
    unfold ords_distinct in *. cbn [drop_key]. cbn [map] in Hnd.
    apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct ((ord y =? o) && (rank y =? k)); [exact Hnd |].
    cbn [map]. apply NoDup_cons_iff. split; [| apply IH; exact Hnd].
    intros Hin. apply Hnot. apply in_map_iff in Hin as [z [Hz Hzin]].
    rewrite <- Hz. apply in_map. eapply drop_key_incl. exact Hzin.
  Qed.

  Lemma drop_key_absent :
    forall (E : Type) (ord rank : E -> nat) l h,
      ords_distinct ord l -> In h l -> ~ In (ord h) (map ord (drop_key ord rank (ord h) (rank h) l)).
  Proof.
    intros E ord rank l. induction l as [| y rest IH]; intros h Hnd Hin; [destruct Hin |].
    pose proof Hnd as Hnd0. unfold ords_distinct in Hnd. cbn [map] in Hnd.
    apply NoDup_cons_iff in Hnd as [Hnot Hnd]. cbn [drop_key].
    destruct ((ord y =? ord h) && (rank y =? rank h)) eqn:Ek.
    - apply andb_true_iff in Ek as [Eo _]. apply Nat.eqb_eq in Eo. rewrite <- Eo. exact Hnot.
    - destruct Hin as [Hin | Hin]; [subst y; rewrite !Nat.eqb_refl in Ek; discriminate |].
      cbn [map]. intros [H | H].
      + assert (Hy : y = h)
          by (apply (ords_inj E ord (y :: rest) y h Hnd0); [left; reflexivity | right; exact Hin | exact H]).
        subst y. rewrite !Nat.eqb_refl in Ek. discriminate.
      + exact (IH h Hnd Hin H).
  Qed.

  (* The removal of a key that no entry holds keeps the list. *)
  Lemma drop_key_missing :
    forall (E : Type) (ord rank : E -> nat) o k l, ~ In o (map ord l) -> drop_key ord rank o k l = l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; intros Hnot; [reflexivity |].
    cbn [drop_key]. cbn [map] in Hnot.
    destruct ((ord y =? o) && (rank y =? k)) eqn:Ek.
    - exfalso. apply andb_true_iff in Ek as [Eo _]. apply Nat.eqb_eq in Eo.
      apply Hnot. left. exact Eo.
    - f_equal. apply IH. intros H. apply Hnot. right. exact H.
  Qed.

  (* The invariant of the settle loop: the orders of each side are distinct. A
     record in flight is not persistent, and its order is not stored. *)
  Definition settle_inv (st : sstate) (x : option record) : Prop :=
    ords_distinct r_ord (s_records st) /\ ords_distinct w_ord (s_waiters st) /\
    match x with
    | Some r => r_pers r = false /\ ~ In (r_ord r) (map r_ord (s_records st))
    | None => True
    end.

  Lemma view_distinct : forall st x, settle_inv st x -> ords_distinct r_ord (view st x).
  Proof.
    intros st [r |] [Hr [_ Hx]]; cbn [view]; [| exact Hr].
    destruct Hx as [_ Hnot]. unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption.
  Qed.

  (* The fire of a settle round that does not land first binds the record in
     flight, or a stored record when nothing is in flight. *)
  Lemma next_fire_bound :
    forall st x w r b,
      settle_inv st x -> next_fire st x = Some (w, r, b) -> lands_first x b = false ->
      match x with Some r0 => r = r0 | None => In r (s_records st) end.
  Proof.
    intros st x w r b [Hr [Hw Hx]] Hnf Hl.
    destruct (next_fire_some st x w r b Hnf) as [_ [_ [Hf [_ Hb]]]].
    destruct (fireable_spec _ _ _ Hf) as [Hm _].
    destruct (first_match_spec _ _ _ _ Hm) as [Hin _].
    destruct x as [r0 |]; cbn [view] in Hin; [| exact Hin].
    unfold lands_first in Hl. cbn [has andb] in Hl. apply negb_false_iff in Hl. subst b.
    destruct Hin as [Hin | Hin]; [symmetry; exact Hin |].
    exfalso. destruct Hx as [_ Hnot]. apply Hnot. symmetry in Hb.
    unfold rec_key_eqb in Hb. apply andb_true_iff in Hb as [Ho _]. apply Nat.eqb_eq in Ho.
    rewrite <- Ho. apply in_map. exact Hin.
  Qed.

  (* The records after the staged operations of one fire of the settle. *)
  Lemma settle_step_records :
    forall st x w r,
      s_records (run st (lift_ops x r ++ [SFireProduce w r])) =
      match x with
      | Some _ => s_records st
      | None => if r_pers r then s_records st else drop_record r (s_records st)
      end.
  Proof.
    intros st x w r. unfold lift_ops.
    destruct x as [r0 |]; [| destruct (r_pers r)]; cbn [app run fold_left apply];
      destruct (w_pers w); reflexivity.
  Qed.

  Lemma settle_step_waiters :
    forall st x w r,
      s_waiters (run st (lift_ops x r ++ [SFireProduce w r])) =
      if w_pers w then s_waiters st else drop_waiter w (s_waiters st).
  Proof.
    intros st x w r. unfold lift_ops.
    destruct x as [r0 |]; [| destruct (r_pers r)]; cbn [app run fold_left apply];
      destruct (w_pers w); reflexivity.
  Qed.

  (* A record in flight after a fire is the record of the fire, out of the
     store, and the fire did not spend it. *)
  Lemma flight_after_some :
    forall x r spent r1,
      flight_after x r spent = Some r1 -> r1 = r /\ flying x r = true /\ spent = false.
  Proof.
    intros x r spent r1 H. unfold flight_after in H.
    destruct (flying x r) eqn:Ef; destruct spent; cbn [andb negb] in H; try discriminate.
    all: injection H as <-; split; [reflexivity | split; reflexivity].
  Qed.

  Lemma settle_step_inv :
    forall st x w r b spent,
      settle_inv st x -> next_fire st x = Some (w, r, b) -> lands_first x b = false ->
      settle_inv (run st (lift_ops x r ++ [SFireProduce w r])) (flight_after x r spent).
  Proof.
    intros st x w r b spent Hinv Hnf Hl. pose proof (next_fire_bound st x w r b Hinv Hnf Hl) as Hrx.
    destruct Hinv as [Hr [Hw Hx]]. split; [| split].
    - rewrite settle_step_records. destruct x; [exact Hr |].
      destruct (r_pers r); [exact Hr | apply drop_key_distinct; exact Hr].
    - rewrite settle_step_waiters. destruct (w_pers w); [exact Hw | apply drop_key_distinct; exact Hw].
    - destruct (flight_after x r spent) as [r1 |] eqn:Ef; [| exact I].
      destruct (flight_after_some x r spent r1 Ef) as [Hr1 [Hfly _]]. subst r1.
      rewrite settle_step_records. destruct x as [r0 |].
      + subst r0. exact Hx.
      + unfold flying in Hfly. destruct (r_pers r) eqn:Ep; [discriminate |].
        split; [reflexivity |]. apply drop_key_absent; [exact Hr | exact Hrx].
  Qed.

  Lemma land_inv : forall st x, settle_inv st x -> settle_inv (run st (land x)) None.
  Proof.
    intros st [r |] [Hr [Hw Hx]]; cbn [land run fold_left apply s_records s_waiters].
    - destruct Hx as [_ Hnot]. split; [| split; [exact Hw | exact I]].
      unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption.
    - split; [exact Hr | split; [exact Hw | exact I]].
  Qed.

  (* The re-store of a stored record puts it in front, and the records are a
     permutation of the records before. *)
  Lemma drop_record_perm :
    forall l h, ords_distinct r_ord l -> In h l -> Permutation (h :: drop_record h l) l.
  Proof.
    induction l as [| x rest IH]; intros h Hnd Hin; [destruct Hin |].
    unfold drop_record. cbn [drop_key].
    destruct ((r_ord x =? r_ord h) && (r_rank x =? r_rank h)) eqn:Ek.
    - apply andb_true_iff in Ek as [Eo _]. apply Nat.eqb_eq in Eo.
      assert (Hx : x = h)
        by (apply (ords_inj record r_ord (x :: rest) x h Hnd); [left; reflexivity | exact Hin | exact Eo]).
      subst x. reflexivity.
    - destruct Hin as [Hin | Hin].
      + subst x. rewrite !Nat.eqb_refl in Ek. discriminate.
      + unfold ords_distinct in Hnd. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [_ Hnd].
        apply (perm_trans (perm_swap x h (drop_record h rest))).
        apply perm_skip. apply IH; [exact Hnd | exact Hin].
  Qed.

  (* ── 10a. The heads ignore the list order ──────────────────────────── *)

  Lemma filter_perm :
    forall (A : Type) (f : A -> bool) l1 l2, Permutation l1 l2 -> Permutation (filter f l1) (filter f l2).
  Proof.
    intros A f l1 l2 H. induction H as [| x l1 l2 H IH | x y l | l1 l2 l3 H1 IH1 H2 IH2]; cbn [filter].
    - constructor.
    - destruct (f x); [apply perm_skip |]; exact IH.
    - destruct (f x), (f y); [apply perm_swap | apply Permutation_refl | apply Permutation_refl
                               | apply Permutation_refl].
    - eapply perm_trans; eassumption.
  Qed.

  Lemma ords_distinct_perm :
    forall (E : Type) (ord : E -> nat) l1 l2,
      Permutation l1 l2 -> ords_distinct ord l1 -> ords_distinct ord l2.
  Proof.
    intros E ord l1 l2 Hp Hnd. unfold ords_distinct in *.
    eapply Permutation_NoDup; [apply Permutation_map; exact Hp | exact Hnd].
  Qed.

  Lemma ords_distinct_filter :
    forall (E : Type) (ord : E -> nat) (f : E -> bool) l,
      ords_distinct ord l -> ords_distinct ord (filter f l).
  Proof.
    intros E ord f l. induction l as [| e rest IH]; intros Hnd; [exact Hnd |].
    unfold ords_distinct in *. cbn [map filter] in *. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct (f e); [| exact (IH Hnd)].
    cbn [map]. apply NoDup_cons; [| exact (IH Hnd)].
    intros Hin. apply Hnot. apply in_map_iff in Hin as [y [Hy Hyin]].
    rewrite <- Hy. apply in_map. apply filter_In in Hyin as [Hyin _]. exact Hyin.
  Qed.

  (* Distinct orders give distinct keys for every key that determines the
     entry in the list. *)
  Lemma nodup_key_of_ords :
    forall (E K : Type) (ord : E -> nat) (key : E -> K) l,
      ords_distinct ord l -> (forall a b, In a l -> In b l -> key a = key b -> a = b) ->
      NoDup (map key l).
  Proof.
    intros E K ord key l. induction l as [| e rest IH]; intros Hnd Hinj; [constructor |].
    unfold ords_distinct in Hnd. cbn [map] in *. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    apply NoDup_cons.
    - intros Hin. apply in_map_iff in Hin as [b [Hb Hbin]].
      assert (He : e = b) by (apply Hinj; [left; reflexivity | right; exact Hbin | symmetry; exact Hb]).
      subst b. apply Hnot. apply in_map. exact Hbin.
    - apply IH; [exact Hnd |]. intros a b Ha Hb. apply Hinj; right; assumption.
  Qed.

  Lemma keys_of_ords :
    forall (E : Type) (ord rank : E -> nat) l, ords_distinct ord l -> keys_distinct ord rank l.
  Proof. intros E ord rank l Hnd. unfold keys_distinct. apply nodup_pair_of_fst. exact Hnd. Qed.

  Lemma record_head_perm :
    forall l1 l2 lvl, Permutation l1 l2 -> ords_distinct r_ord l1 ->
      record_head l1 lvl = record_head l2 lvl.
  Proof.
    intros l1 l2 lvl Hp Hnd. unfold record_head. apply least_perm.
    - apply filter_perm. exact Hp.
    - apply keys_of_ords. apply ords_distinct_filter. exact Hnd.
  Qed.

  Lemma waiter_head_perm :
    forall l1 l2 lane, Permutation l1 l2 -> ords_distinct w_ord l1 ->
      waiter_head l1 lane = waiter_head l2 lane.
  Proof.
    intros l1 l2 lane Hp Hnd. unfold waiter_head. apply least_perm.
    - apply filter_perm. exact Hp.
    - apply keys_of_ords. apply ords_distinct_filter. exact Hnd.
  Qed.

  Lemma is_record_head_perm :
    forall l1 l2 r, Permutation l1 l2 -> ords_distinct r_ord l1 ->
      is_record_head l1 r = is_record_head l2 r.
  Proof.
    intros l1 l2 r Hp Hnd. unfold is_record_head. rewrite (record_head_perm l1 l2 _ Hp Hnd).
    reflexivity.
  Qed.

  Lemma is_waiter_head_perm :
    forall l1 l2 w, Permutation l1 l2 -> ords_distinct w_ord l1 ->
      is_waiter_head l1 w = is_waiter_head l2 w.
  Proof.
    intros l1 l2 w Hp Hnd. unfold is_waiter_head. rewrite (waiter_head_perm l1 l2 _ Hp Hnd).
    reflexivity.
  Qed.

  (* With distinct orders, a level has one head. *)
  Lemma level_heads_inj :
    forall l a b, ords_distinct r_ord l -> In a l -> In b l ->
      is_record_head l a = true -> is_record_head l b = true -> r_lvl a = r_lvl b -> a = b.
  Proof.
    intros l a b Hnd Ha Hb Hha Hhb Hlvl.
    apply (is_record_head_iff l a Hnd Ha) in Hha. apply (is_record_head_iff l b Hnd Hb) in Hhb.
    rewrite Hlvl, Hhb in Hha. injection Hha as ->. reflexivity.
  Qed.

  Lemma lvl_key_inj :
    forall a b : option nat, lvl_class a = lvl_class b -> lvl_index a = lvl_index b -> a = b.
  Proof. intros [x |] [y |]; cbn; intros H1 H2; try discriminate; [subst | ]; reflexivity. Qed.

  Lemma first_match_perm :
    forall l1 l2 lane pat, Permutation l1 l2 -> ords_distinct r_ord l1 ->
      first_match l1 lane pat = first_match l2 lane pat.
  Proof.
    intros l1 l2 lane pat Hp Hnd. unfold first_match.
    rewrite (filter_ext (fun r => is_record_head l2 r && sees lane (r_lvl r) && spatial pat (r_val r))
                        (fun r => is_record_head l1 r && sees lane (r_lvl r) && spatial pat (r_val r))).
    2: { intros r. rewrite (is_record_head_perm l1 l2 r Hp Hnd). reflexivity. }
    apply least_perm; [apply filter_perm; exact Hp |].
    unfold keys_distinct. apply (nodup_key_of_ords record (nat * nat) r_ord);
      [apply ords_distinct_filter; exact Hnd |].
    intros a b Ha Hb Hab. injection Hab as H1 H2.
    apply filter_In in Ha as [Ha Hfa]. apply filter_In in Hb as [Hb Hfb].
    apply andb_true_iff in Hfa as [Hfa _]. apply andb_true_iff in Hfa as [Hha _].
    apply andb_true_iff in Hfb as [Hfb _]. apply andb_true_iff in Hfb as [Hhb _].
    apply (level_heads_inj l1 a b Hnd Ha Hb Hha Hhb). apply lvl_key_inj; assumption.
  Qed.

  Lemma fireable_perm :
    forall l1 l2 w, Permutation l1 l2 -> ords_distinct r_ord l1 -> fireable l1 w = fireable l2 w.
  Proof.
    intros l1 l2 w Hp Hnd. unfold fireable. rewrite (first_match_perm l1 l2 _ _ Hp Hnd).
    reflexivity.
  Qed.

  (* ── 11. The settle fires the oldest fireable head (readiness review S4) ── *)

  (* A settle fire of w on r: the waiter is a waiter-lane head, and it fires r
     on the view where r is in flight. No older waiter-lane head can fire on
     that view. The view puts r in front of the stored records without r. *)
  Definition oldest_at (st : sstate) (op : staged) : Prop :=
    match op with
    | SFireProduce w r =>
        let v := r :: drop_record r (s_records st) in
        In w (s_waiters st) /\ is_waiter_head (s_waiters st) w = true /\ fireable v w = Some r /\
        (forall u, In u (s_waiters st) -> is_waiter_head (s_waiters st) u = true ->
                   fireable v u <> None -> key_le w_ord w_rank w u)
    | _ => True
    end.

  Fixpoint oldest_trace (st : sstate) (ops : list staged) : Prop :=
    match ops with
    | [] => True
    | op :: rest => oldest_at st op /\ oldest_trace (apply st op) rest
    end.

  Lemma oldest_trace_app :
    forall a b st, oldest_trace st a -> oldest_trace (run st a) b -> oldest_trace st (a ++ b).
  Proof.
    induction a as [| op rest IH]; intros b st Ha Hb; [exact Hb |].
    destruct Ha as [Hop Hrest]. split; [exact Hop |]. apply IH; [exact Hrest | exact Hb].
  Qed.

  Lemma land_oldest : forall st x, oldest_trace st (land x).
  Proof. intros st [r |]; cbn; [split; exact I | exact I]. Qed.

  (* The view of the fire of a settle round is a permutation of the view of
     the round. *)
  Lemma settle_step_view :
    forall st x w r b,
      settle_inv st x -> next_fire st x = Some (w, r, b) -> lands_first x b = false ->
      s_waiters (run st (lift_ops x r)) = s_waiters st /\
      Permutation (view st x) (r :: drop_record r (s_records (run st (lift_ops x r)))).
  Proof.
    intros st x w r b Hinv Hnf Hl. pose proof (next_fire_bound st x w r b Hinv Hnf Hl) as Hrx.
    destruct Hinv as [Hr [_ Hx]]. unfold lift_ops.
    destruct x as [r0 |].
    - subst r0. cbn [run fold_left view]. split; [reflexivity |].
      destruct Hx as [_ Hnot]. unfold drop_record. rewrite (drop_key_missing _ _ _ _ _ _ Hnot).
      apply Permutation_refl.
    - cbn [view]. destruct (r_pers r); cbn [run fold_left apply s_records s_waiters];
        split; try reflexivity.
      + apply Permutation_sym. apply drop_record_perm; assumption.
      + unfold drop_record at 1.
        rewrite (drop_key_missing _ _ _ _ _ _ (drop_key_absent _ r_ord r_rank _ r Hr Hrx)).
        apply Permutation_sym. apply drop_record_perm; assumption.
  Qed.

  Lemma settle_step_oldest :
    forall st x w r b,
      settle_inv st x -> next_fire st x = Some (w, r, b) -> lands_first x b = false ->
      oldest_trace st (lift_ops x r ++ [SFireProduce w r]).
  Proof.
    intros st x w r b Hinv Hnf Hl.
    destruct (settle_step_view st x w r b Hinv Hnf Hl) as [Hws Hperm].
    pose proof (view_distinct st x Hinv) as Hd.
    destruct (next_fire_some st x w r b Hnf) as [Hw [Hhead [Hf [Hmin _]]]].
    apply oldest_trace_app;
      [unfold lift_ops; destruct x; [exact I | destruct (r_pers r); cbn; [exact I | split; exact I]] |].
    cbn [oldest_trace oldest_at]. split; [| exact I]. rewrite Hws.
    rewrite <- (fireable_perm _ _ w Hperm Hd).
    split; [exact Hw | split; [exact Hhead | split; [exact Hf |]]].
    intros u Hu Hhu Hfu. apply Hmin; [exact Hu | exact Hhu |].
    rewrite (fireable_perm _ _ u Hperm Hd). exact Hfu.
  Qed.

  Lemma settle_oldest :
    forall fuel fired st x, settle_inv st x -> oldest_trace st (z_ops (settle_loop fuel fired st x)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x Hinv; cbn [settle_loop]; [apply land_oldest |].
    destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf; [| apply land_oldest].
    destruct (lands_first x b) eqn:Hl.
    - cbn [z_ops]. apply oldest_trace_app; [apply land_oldest |]. apply IH. apply land_inv.
      exact Hinv.
    - destruct (at_cap fired); [apply land_oldest |].
      pose proof (settle_step_oldest st x w r b Hinv Hnf Hl) as Hstep.
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        cbn [z_ops]; apply oldest_trace_app; try exact Hstep;
        solve [apply land_oldest | apply IH; apply (settle_step_inv st x w r b); assumption].
  Qed.

  (* The candidate heads of a datum. *)
  Definition candidate (ws : list waiter) (r : record) (refused : bool * bool) (u : waiter) : Prop :=
    cand1 ws r (fst refused) = Some u \/ cand2 ws (snd refused) = Some u.

  Lemma filter_keep :
    forall (A : Type) (f g : A -> bool) l,
      (forall x, In x l -> g x = false -> f x = false) -> filter f (filter g l) = filter f l.
  Proof.
    intros A f g l. induction l as [| a rest IH]; intros H; [reflexivity |].
    cbn [filter]. destruct (g a) eqn:Eg.
    - cbn [filter]. destruct (f a); [f_equal |]; apply IH; intros x Hx; apply H; right; exact Hx.
    - rewrite (H a (or_introl eq_refl) Eg). apply IH. intros x Hx. apply H. right. exact Hx.
  Qed.

  Lemma drop_key_filter :
    forall (E : Type) (ord rank : E -> nat) o k l,
      ords_distinct ord l ->
      drop_key ord rank o k l = filter (fun x => negb ((ord x =? o) && (rank x =? k))) l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; intros Hnd; [reflexivity |].
    unfold ords_distinct in Hnd. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    cbn [drop_key filter].
    destruct ((ord y =? o) && (rank y =? k)) eqn:Ey; cbn [negb].
    - symmetry. rewrite <- (filter_true rest) at 2. apply filter_ext_in.
      intros z Hz. apply andb_true_iff in Ey as [Eo _]. apply Nat.eqb_eq in Eo.
      destruct ((ord z =? o) && (rank z =? k)) eqn:Ez; [| reflexivity].
      apply andb_true_iff in Ez as [Ezo _]. apply Nat.eqb_eq in Ezo.
      exfalso. apply Hnot. rewrite Eo, <- Ezo. apply in_map. exact Hz.
    - f_equal. apply IH. exact Hnd.
  Qed.

  (* The older waiter fires first in a produce plan (decision 38), and a
     refusal moves on (decision 37). The candidates of a datum are the head of
     the waiter queue of its level and the head of the unrestricted queue. The
     plan fires a candidate that fits the datum (the pattern, then the guard),
     and no older candidate fits it. When the plan fires none, no candidate
     fits. The fire of a waiter keeps the head of every other lane. So a
     refused lane keeps its refused head, and the plan does not test it again. *)
  Lemma offer_pick_older_first :
    (forall ws r refused w refused', offer_pick ws r refused = (Some w, refused') ->
       candidate ws r refused w /\ fits_datum w (r_val r) = true /\
       forall u, candidate ws r refused u -> fits_datum u (r_val r) = true -> w_ord w <= w_ord u) /\
    (forall ws r refused refused', offer_pick ws r refused = (None, refused') ->
       forall u, candidate ws r refused u -> fits_datum u (r_val r) = false) /\
    (forall ws w lane, ords_distinct w_ord ws -> In w ws -> w_lane w <> lane ->
       waiter_head (drop_waiter w ws) lane = waiter_head ws lane).
  Proof.
    split; [| split].
    - intros ws r [ref1 ref2] w refused' H. unfold offer_pick in H. unfold candidate.
      cbn [fst snd] in *.
      destruct (cand1 ws r ref1) as [w1 |]; destruct (cand2 ws ref2) as [w2 |].
      + destruct (w_ord w2 <? w_ord w1) eqn:Elt.
        * apply Nat.ltb_lt in Elt. destruct (fits_datum w2 (r_val r)) eqn:F2.
          -- injection H as Hw _. subst w. split; [right; reflexivity | split; [exact F2 |]].
             intros u [Hu | Hu] Fu; injection Hu as <-; lia.
          -- destruct (fits_datum w1 (r_val r)) eqn:F1; [| discriminate].
             injection H as Hw _. subst w. split; [left; reflexivity | split; [exact F1 |]].
             intros u [Hu | Hu] Fu; injection Hu as <-; [lia | rewrite F2 in Fu; discriminate].
        * apply Nat.ltb_ge in Elt. destruct (fits_datum w1 (r_val r)) eqn:F1.
          -- injection H as Hw _. subst w. split; [left; reflexivity | split; [exact F1 |]].
             intros u [Hu | Hu] Fu; injection Hu as <-; lia.
          -- destruct (fits_datum w2 (r_val r)) eqn:F2; [| discriminate].
             injection H as Hw _. subst w. split; [right; reflexivity | split; [exact F2 |]].
             intros u [Hu | Hu] Fu; injection Hu as <-; [rewrite F1 in Fu; discriminate | lia].
      + destruct (fits_datum w1 (r_val r)) eqn:F1; [| discriminate].
        injection H as Hw _. subst w. split; [left; reflexivity | split; [exact F1 |]].
        intros u [Hu | Hu] Fu; [injection Hu as <-; lia | discriminate].
      + destruct (fits_datum w2 (r_val r)) eqn:F2; [| discriminate].
        injection H as Hw _. subst w. split; [right; reflexivity | split; [exact F2 |]].
        intros u [Hu | Hu] Fu; [discriminate | injection Hu as <-; lia].
      + discriminate.
    - intros ws r [ref1 ref2] refused' H u Hu. unfold offer_pick in H. unfold candidate in Hu.
      cbn [fst snd] in *.
      destruct (cand1 ws r ref1) as [w1 |]; destruct (cand2 ws ref2) as [w2 |].
      + destruct (w_ord w2 <? w_ord w1);
          destruct (fits_datum w2 (r_val r)) eqn:F2; destruct (fits_datum w1 (r_val r)) eqn:F1;
          try discriminate; destruct Hu as [Hu | Hu]; injection Hu as <-; assumption.
      + destruct (fits_datum w1 (r_val r)) eqn:F1; [discriminate |].
        destruct Hu as [Hu | Hu]; [injection Hu as <-; exact F1 | discriminate].
      + destruct (fits_datum w2 (r_val r)) eqn:F2; [discriminate |].
        destruct Hu as [Hu | Hu]; [discriminate | injection Hu as <-; exact F2].
      + destruct Hu as [Hu | Hu]; discriminate.
    - intros ws w lane Hnd Hin Hne. unfold waiter_head, drop_waiter.
      rewrite (drop_key_filter waiter w_ord w_rank (w_ord w) (w_rank w) ws Hnd).
      f_equal. apply filter_keep. intros x Hx Hk. apply negb_false_iff in Hk.
      apply andb_true_iff in Hk as [Ho _]. apply Nat.eqb_eq in Ho.
      assert (Hxw : x = w) by exact (ords_inj waiter w_ord ws x w Hnd Hx Hin Ho).
      subst x. destruct (lvl_eqb (w_lane w) lane) eqn:El; [| reflexivity].
      apply lvl_eqb_iff in El. contradiction.
  Qed.

  (* Headline 6. The oldest fireable waiter fires first (PqOldestFirst,
     readiness review S4, decision 38). Each fire of a settle fires a head of a
     waiter lane on the view where its record is in flight. No older head of a
     waiter lane can fire on that view. This holds for every fire count, so for
     the settle before the plan, the settle after the plan and a settle job.
     The premise of the settle part is wf: distinct orders on each side. A
     produce plan fires the older candidate head that fits its datum, and a
     refusal moves on to the other candidate (decision 37). The fire of a
     waiter keeps the head of every other lane. *)
  Theorem pq_settle_oldest_first :
    (forall fired st,
       ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) ->
       oldest_trace st (z_ops (settle_at fired st))) /\
    (forall ws r refused w refused', offer_pick ws r refused = (Some w, refused') ->
       candidate ws r refused w /\ fits_datum w (r_val r) = true /\
       forall u, candidate ws r refused u -> fits_datum u (r_val r) = true -> w_ord w <= w_ord u) /\
    (forall ws r refused refused', offer_pick ws r refused = (None, refused') ->
       forall u, candidate ws r refused u -> fits_datum u (r_val r) = false) /\
    (forall ws w lane, ords_distinct w_ord ws -> In w ws -> w_lane w <> lane ->
       waiter_head (drop_waiter w ws) lane = waiter_head ws lane).
  Proof.
    split; [| exact offer_pick_older_first].
    intros fired st Hr Hw. unfold settle_at. apply settle_oldest.
    split; [exact Hr | split; [exact Hw | exact I]].
  Qed.

  (* Headline 4. A restricted receive waits in the waiter queue of its level
     (decision 38). The waiter queue of the level p sees exactly the level p.
     So a waiter of that queue fires only on the head of the level p. A fresh
     receive with priority p joins the tail of that queue: it parks when the
     queue has a head. *)
  Theorem pq_restricted_waiter_in_level_queue :
    (forall p lvl, sees (Some p) lvl = true <-> lvl = Some p) /\
    (forall l w h p, fireable l w = Some h -> w_lane w = Some p ->
       In h l /\ r_lvl h = Some p /\ is_record_head l h = true) /\
    (forall f s k u, waiter_head (s_waiters s) (w_lane k) = Some u ->
       fresh_receive k f s = mkPlanned [SPark k] PDone).
  Proof.
    split; [exact sees_level | split].
    - intros l w h p Hf Hl. destruct (fireable_spec l w h Hf) as [Hm _].
      destruct (first_match_spec l (w_lane w) (w_pat w) h Hm) as [Hin [Hh [Hs _]]].
      rewrite Hl in Hs. apply sees_level in Hs.
      split; [exact Hin | split; [exact Hs | exact Hh]].
    - intros f s k u Hu. unfold fresh_receive. rewrite Hu. reflexivity.
  Qed.

  (* Headline 5. An unrestricted receive takes the first matching level
     (decision 38). The unrestricted queue sees every level, so a receive
     binds the first level head, in ascending level order, whose payload
     matches its pattern. It checks its guard once, on that head: a refusal
     parks it, although a later level head may pass the guard. With no
     matching level head, the receive parks. The rule of the guard holds for
     the receive of every lane. *)
  Theorem pq_unrestricted_first_matching_level :
    (forall lvl, sees None lvl = true) /\
    (forall l pat h, first_match l None pat = Some h ->
       In h l /\ is_record_head l h = true /\ spatial pat (r_val h) = true /\
       (forall x, In x l -> is_record_head l x = true -> spatial pat (r_val x) = true ->
                  lvl_le (r_lvl h) (r_lvl x) = true)) /\
    (forall fuel fired st k,
       (first_match (s_records st) (w_lane k) (w_pat k) = None ->
          receive_loop (S fuel) fired st k = mkPlanned [SPark k] PDone) /\
       (forall h, first_match (s_records st) (w_lane k) (w_pat k) = Some h ->
          admits (w_guard k) (r_val h) = false ->
          receive_loop (S fuel) fired st k = mkPlanned [SPark k] PDone) /\
       (forall h, first_match (s_records st) (w_lane k) (w_pat k) = Some h ->
          admits (w_guard k) (r_val h) = true ->
          exists rest, p_ops (receive_loop (S fuel) fired st k) = consume_fire k h ++ rest)).
  Proof.
    split; [exact sees_any | split].
    - intros l pat h Hm. destruct (first_match_spec l None pat h Hm) as [Hin [Hh [_ [Hsp Hmin]]]].
      split; [exact Hin | split; [exact Hh | split; [exact Hsp |]]].
      intros x Hx Hxh Hxsp. exact (Hmin x Hx Hxh (sees_any (r_lvl x)) Hxsp).
    - intros fuel fired st k. split; [| split].
      + intros H. cbn [receive_loop]. rewrite H. reflexivity.
      + intros h H Ha. cbn [receive_loop]. rewrite H, Ha. reflexivity.
      + intros h H Ha. cbn [receive_loop]. rewrite H, Ha.
        destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)); cbn [p_ops];
          solve [exists []; rewrite app_nil_r; reflexivity | eexists; reflexivity].
  Qed.

  (* ── 12. FIFO: every fire binds lane heads ─────────────────────────────── *)

  (* A fire binds heads in the view where it is staged. The waiter of a
     produce-side fire is a waiter-lane head. Its record has a key at most the
     key of every stored record of its level. The record of a consume-side fire
     is a level head. Its continuation has a key at most the key of every
     waiter of its lane. A lift takes a level head. *)
  Definition fifo_at (st : sstate) (op : staged) : Prop :=
    match op with
    | SFireProduce w r =>
        is_waiter_head (s_waiters st) w = true /\
        forall h, In h (s_records st) -> r_lvl h = r_lvl r -> key_le r_ord r_rank r h
    | SFireConsume k h =>
        is_record_head (s_records st) h = true /\
        forall u, In u (s_waiters st) -> w_lane u = w_lane k -> key_le w_ord w_rank k u
    | SLift h => is_record_head (s_records st) h = true
    | SStore _ | SPark _ | SRestore _ => True
    end.

  Fixpoint fifo_trace (st : sstate) (ops : list staged) : Prop :=
    match ops with
    | [] => True
    | op :: rest => fifo_at st op /\ fifo_trace (apply st op) rest
    end.

  Lemma fifo_trace_app :
    forall a b st, fifo_trace st a -> fifo_trace (run st a) b -> fifo_trace st (a ++ b).
  Proof.
    induction a as [| op rest IH]; intros b st Ha Hb; [exact Hb |].
    destruct Ha as [Hop Hrest]. split; [exact Hop |]. apply IH; [exact Hrest | exact Hb].
  Qed.

  Lemma land_fifo : forall st x, fifo_trace st (land x).
  Proof. intros st [r |]; cbn; [split; exact I | exact I]. Qed.

  (* A level head has a key at most the key of every record of its level. *)
  Lemma record_head_le :
    forall l r, is_record_head l r = true ->
      forall h, In h l -> r_lvl h = r_lvl r -> key_le r_ord r_rank r h.
  Proof.
    intros l r Hr h Hh Hl. unfold is_record_head in Hr.
    destruct (record_head l (r_lvl r)) as [h0 |] eqn:E; [| discriminate].
    unfold rec_key_eqb in Hr. apply andb_true_iff in Hr as [Ho Hk].
    apply Nat.eqb_eq in Ho. apply Nat.eqb_eq in Hk.
    destruct (record_head_spec l (r_lvl r) h0 E) as [_ [_ Hmin]].
    pose proof (Hmin h Hh Hl) as Gk. unfold key_le in *. rewrite <- Ho, <- Hk. exact Gk.
  Qed.

  Lemma waiter_head_is_head :
    forall l lane w, waiter_head l lane = Some w -> is_waiter_head l w = true.
  Proof.
    intros l lane w H. destruct (waiter_head_spec l lane w H) as [_ [Hl _]].
    unfold is_waiter_head. rewrite Hl, H. unfold wtr_key_eqb. rewrite !Nat.eqb_refl. reflexivity.
  Qed.

  Lemma candidate_head :
    forall ws r refused u, candidate ws r refused u -> is_waiter_head ws u = true.
  Proof.
    intros ws r [ref1 ref2] u [H | H]; cbn [fst snd] in H; unfold cand1, cand2 in H.
    - destruct ref1; [discriminate |]. destruct (r_lvl r) as [p |]; [| discriminate].
      exact (waiter_head_is_head _ _ _ H).
    - destruct ref2; [discriminate |]. exact (waiter_head_is_head _ _ _ H).
  Qed.

  (* Every fire of a settle binds lane heads of the view. *)
  Lemma settle_step_fifo :
    forall st x w r b, next_fire st x = Some (w, r, b) ->
      fifo_trace st (lift_ops x r ++ [SFireProduce w r]).
  Proof.
    intros st x w r b Hnf.
    destruct (next_fire_some st x w r b Hnf) as [_ [Hhead [Hf _]]].
    destruct (fireable_spec _ _ _ Hf) as [Hm _].
    destruct (first_match_spec _ _ _ _ Hm) as [_ [Hrh _]].
    pose proof (record_head_le _ _ Hrh) as Hle.
    unfold lift_ops. destruct x as [r0 |].
    - cbn [app fifo_trace fifo_at]. split; [| exact I]. split; [exact Hhead |].
      intros h Hh Hl. apply Hle; [cbn [view]; right; exact Hh | exact Hl].
    - cbn [view] in Hrh, Hle. destruct (r_pers r).
      + cbn [app fifo_trace fifo_at]. split; [| exact I]. split; [exact Hhead | exact Hle].
      + cbn [app fifo_trace fifo_at apply s_records s_waiters]. split; [exact Hrh |].
        split; [| exact I]. split; [exact Hhead |].
        intros h Hh Hl. apply Hle; [unfold drop_record in Hh; eapply drop_key_incl; exact Hh | exact Hl].
  Qed.

  Lemma settle_fifo : forall fuel fired st x, fifo_trace st (z_ops (settle_loop fuel fired st x)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x; cbn [settle_loop]; [apply land_fifo |].
    destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf; [| apply land_fifo].
    destruct (lands_first x b).
    - cbn [z_ops]. apply fifo_trace_app; [apply land_fifo | apply IH].
    - destruct (at_cap fired); [apply land_fifo |].
      pose proof (settle_step_fifo st x w r b Hnf) as Hstep.
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        cbn [z_ops]; apply fifo_trace_app; try exact Hstep; solve [apply land_fifo | apply IH].
  Qed.

  Lemma fire_produce_records : forall st w r, s_records (apply st (SFireProduce w r)) = s_records st.
  Proof. intros st w r. cbn [apply]. destruct (w_pers w); reflexivity. Qed.

  (* Every fire of the produce plan binds a candidate head, and the datum is
     at the head of its level. *)
  Lemma offer_loop_fifo :
    forall fuel fired st r refused,
      (forall h, In h (s_records st) -> r_lvl h = r_lvl r -> key_le r_ord r_rank r h) ->
      fifo_trace st (p_ops (offer_loop fuel fired st r refused)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r refused Hr; cbn [offer_loop]; [exact I |].
    destruct (offer_pick (s_waiters st) r refused) as [[w |] refused'] eqn:Hp; [| split; exact I].
    pose proof (proj1 (proj1 offer_pick_older_first _ _ _ _ _ Hp)) as Hc.
    pose proof (candidate_head _ _ _ _ Hc) as Hw.
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [p_ops fifo_trace fifo_at];
      try (split; [split; [exact Hw | exact Hr] | exact I]).
    split; [split; [exact Hw | exact Hr] |].
    apply IH. rewrite fire_produce_records. exact Hr.
  Qed.

  Lemma consume_fire_waiters :
    forall st k h, s_waiters (run st (consume_fire k h)) = s_waiters st.
  Proof.
    intros st k h. unfold consume_fire.
    destruct (w_peek k && negb (r_pers h)); cbn [run fold_left apply];
      destruct (r_pers h); reflexivity.
  Qed.

  Lemma consume_fire_fifo :
    forall st k h, is_record_head (s_records st) h = true ->
      (forall u, In u (s_waiters st) -> w_lane u = w_lane k -> key_le w_ord w_rank k u) ->
      fifo_trace st (consume_fire k h).
  Proof.
    intros st k h Hh Hk. unfold consume_fire.
    destruct (w_peek k && negb (r_pers h)); cbn [fifo_trace fifo_at];
      split; try (split; [exact Hh | exact Hk]); repeat split.
  Qed.

  (* Every fire of the consume plan binds a level head, and the continuation
     is at the head of its lane. *)
  Lemma receive_loop_fifo :
    forall fuel fired st k,
      (forall u, In u (s_waiters st) -> w_lane u = w_lane k -> key_le w_ord w_rank k u) ->
      fifo_trace st (p_ops (receive_loop fuel fired st k)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hk; cbn [receive_loop]; [exact I |].
    destruct (first_match (s_records st) (w_lane k) (w_pat k)) as [h |] eqn:Hm; [| split; exact I].
    destruct (first_match_spec _ _ _ _ Hm) as [_ [Hh _]].
    destruct (admits (w_guard k) (r_val h)); [| split; exact I].
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
      cbn [p_ops]; try (apply consume_fire_fifo; assumption).
    apply fifo_trace_app; [apply consume_fire_fifo; assumption |].
    apply IH. rewrite consume_fire_waiters. exact Hk.
  Qed.

  (* Every fire of an operation binds lane heads, when its plan does from
     every fire count. *)
  Lemma with_settles_fifo :
    forall st pers o plan,
      (forall f, fifo_trace (run st (z_ops (settle_of st)))
                            (p_ops (plan f (run st (z_ops (settle_of st)))))) ->
      fifo_trace st (o_ops (with_settles st pers o plan)).
  Proof.
    intros st pers o plan Hplan. unfold with_settles.
    assert (Hbefore : fifo_trace st (z_ops (settle_of st))) by (unfold settle_of; apply settle_fifo).
    destruct (z_end (settle_of st)); cbn [o_ops]; [| exact Hbefore].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_ops]; [exact Hbefore |].
    match goal with |- context [p_end ?p] => destruct (p_end p) end; cbn [o_ops].
    - apply fifo_trace_app; [exact Hbefore |].
      apply fifo_trace_app; [exact (Hplan _) |].
      rewrite <- run_app. unfold settle_at. apply settle_fifo.
    - apply fifo_trace_app; [exact Hbefore | exact (Hplan _)].
  Qed.

  (* Headline 3. FIFO within each level and each waiter queue (decision 11, the
     rule of the queue discipline). The head of a level and of a waiter queue
     is its entry with the least (ord, rank). A lane has a head exactly when it
     has an entry. Every fire of an operation, of a settle job and of a
     re-entry binds heads. The waiter of a fire is the head of its lane. The
     record of a fire is the head of its level. The incoming side of a plan
     fire is at the head of its lane. A fresh element plans only on an empty
     lane. A re-entry plans only ahead of the head of its lane. So each level
     and each waiter queue leaves in FIFO order. *)
  Theorem pq_fifo_within_level :
    (forall l lvl h, record_head l lvl = Some h ->
       In h l /\ r_lvl h = lvl /\ forall x, In x l -> r_lvl x = lvl -> key_le r_ord r_rank h x) /\
    (forall l lane w, waiter_head l lane = Some w ->
       In w l /\ w_lane w = lane /\ forall x, In x l -> w_lane x = lane -> key_le w_ord w_rank w x) /\
    (forall l lvl, record_head l lvl = None <-> forall x, In x l -> r_lvl x <> lvl) /\
    (forall l lane, waiter_head l lane = None <-> forall x, In x l -> w_lane x <> lane) /\
    (forall st rank v lvl pers, fifo_trace (bumped st) (o_ops (produce st rank v lvl pers))) /\
    (forall st rank pat g lane pers peek,
       fifo_trace (bumped st) (o_ops (consume st rank pat g lane pers peek))) /\
    (forall st, fifo_trace (bumped st) (o_ops (settle_job st))) /\
    (forall st x, fifo_trace (bumped st) (o_ops (reenter_produce st x))) /\
    (forall st k, fifo_trace (bumped st) (o_ops (reenter_consume st k))).
  Proof.
    split; [exact record_head_spec | split; [exact waiter_head_spec |]].
    split; [exact record_head_none | split; [exact waiter_head_none |]].
    split; [| split; [| split; [| split]]].
    - intros st rank v lvl pers. unfold produce. apply with_settles_fifo. intros f.
      unfold fresh_offer.
      match goal with |- context [record_head ?l ?lv] => destruct (record_head l lv) as [h |] eqn:Eh end;
        [split; exact I |].
      apply offer_loop_fifo. intros h Hh Hl. exfalso.
      exact (proj1 (record_head_none _ _) Eh h Hh Hl).
    - intros st rank pat g lane pers peek. unfold consume. apply with_settles_fifo. intros f.
      unfold fresh_receive.
      match goal with |- context [waiter_head ?l ?ln] => destruct (waiter_head l ln) as [w |] eqn:Ew end;
        [split; exact I |].
      apply receive_loop_fifo. intros u Hu Hl. exfalso.
      exact (proj1 (waiter_head_none _ _) Ew u Hu Hl).
    - intros st. unfold settle_job, settle_of. cbn [o_ops]. apply settle_fifo.
    - intros st x. unfold reenter_produce. apply with_settles_fifo. intros f. unfold reenter_offer.
      match goal with |- context [record_head ?l ?lv] => destruct (record_head l lv) as [h |] eqn:Eh end.
      + destruct (r_ord x <? r_ord h) eqn:Hlt; [| split; exact I].
        apply offer_loop_fifo. intros y Hy Hl. apply Nat.ltb_lt in Hlt.
        destruct (record_head_spec _ _ h Eh) as [_ [_ Hmin]].
        pose proof (Hmin y Hy Hl) as Gk. unfold key_le in *. left. lia.
      + first [split; exact I | apply offer_loop_fifo; intros y Hy Hl; exfalso;
                                exact (proj1 (record_head_none _ _) Eh y Hy Hl)].
    - intros st k. unfold reenter_consume. apply with_settles_fifo. intros f. unfold reenter_receive.
      match goal with |- context [waiter_head ?l ?ln] => destruct (waiter_head l ln) as [w |] eqn:Ew end.
      + destruct (w_ord k <? w_ord w) eqn:Hlt; [| split; exact I].
        apply receive_loop_fifo. intros u Hu Hl. apply Nat.ltb_lt in Hlt.
        destruct (waiter_head_spec _ _ w Ew) as [_ [_ Hmin]].
        pose proof (Hmin u Hu Hl) as Gk. unfold key_le in *. left. lia.
      + first [split; exact I | apply receive_loop_fifo; intros u Hu Hl; exfalso;
                                exact (proj1 (waiter_head_none _ _) Ew u Hu Hl)].
  Qed.

  (* ── 13. Quiet settles: head blocking, re-entry, peeks ─────────────────── *)

  (* A settle of a view with INV stages nothing and ends Quiescent, from
     every fire count. *)
  Lemma settle_at_quiet :
    forall fired st, no_pending_match st -> settle_at fired st = mkSettled [] Quiescent.
  Proof.
    intros fired st H. unfold settle_at, settle_fuel. cbn [settle_loop].
    assert (Hnf : next_fire st None = None)
      by (apply next_fire_none_iff; intros w Hw Hh; exact (H w Hw Hh)).
    rewrite Hnf. reflexivity.
  Qed.

  Lemma settle_quiet : forall st, no_pending_match st -> settle_of st = mkSettled [] Quiescent.
  Proof. intros st H. exact (settle_at_quiet 0 st H). Qed.

  (* The outcome of with_settles when both settles are quiet and the plan
     completes. The quiet settle before the plan fires nothing, so the plan
     opens at 0 fires when the cap is above 0. *)
  Lemma with_settles_quiet :
    forall st pers o plan ops,
      0 < cap -> no_pending_match st -> plan 0 st = mkPlanned ops PDone ->
      no_pending_match (run st ops) -> with_settles st pers o plan = mkOutcome ops None.
  Proof.
    intros st pers o plan ops Hcap Hinv Hplan Hafter. unfold with_settles.
    rewrite (settle_quiet st Hinv). cbn [z_end z_ops].
    change (fires []) with 0. rewrite (plan_at_cap_false 0 Hcap), run_nil, Hplan.
    cbn [p_end p_ops app]. rewrite (settle_at_quiet _ (run st ops) Hafter).
    cbn [z_ops z_end settle_next]. rewrite app_nil_r. reflexivity.
  Qed.

  Lemma store_after_bump :
    forall st r,
      run (bumped st) [SStore r] = mkS (r :: s_records st) (s_waiters st) (bump_next (s_counter st)).
  Proof. reflexivity. Qed.

  Lemma park_after_bump :
    forall st k,
      run (bumped st) [SPark k] = mkS (s_records st) (k :: s_waiters st) (bump_next (s_counter st)).
  Proof. reflexivity. Qed.

  Lemma is_record_head_cons_other :
    forall d l y, r_lvl y <> r_lvl d -> is_record_head (d :: l) y = is_record_head l y.
  Proof.
    intros d l y Hne. unfold is_record_head, record_head. cbn [filter].
    destruct (lvl_eqb (r_lvl d) (r_lvl y)) eqn:E; [| reflexivity].
    apply lvl_eqb_iff in E. exfalso. apply Hne. symmetry. exact E.
  Qed.

  Lemma waiter_head_cons_other :
    forall k l lane, w_lane k <> lane -> waiter_head (k :: l) lane = waiter_head l lane.
  Proof.
    intros k l lane Hne. unfold waiter_head. cbn [filter].
    destruct (lvl_eqb (w_lane k) lane) eqn:E; [| reflexivity].
    apply lvl_eqb_iff in E. contradiction.
  Qed.

  Lemma offer_loop_store :
    forall fuel fired st r refused ref', offer_pick (s_waiters st) r refused = (None, ref') ->
      offer_loop (S fuel) fired st r refused = mkPlanned [SStore r] PDone.
  Proof. intros fuel fired st r refused ref' H. simpl. rewrite H. reflexivity. Qed.

  (* A continuation parked on an empty lane, with no level head to fire on,
     keeps INV. *)
  Lemma park_fresh_lane_inv :
    forall st k c, no_pending_match st -> waiter_head (s_waiters st) (w_lane k) = None ->
      fireable (s_records st) k = None -> no_pending_match (mkS (s_records st) (k :: s_waiters st) c).
  Proof.
    intros st k c Hinv Hlane Hk u Hu Hhu. cbn [s_records s_waiters] in *.
    destruct (lvl_eqb (w_lane u) (w_lane k)) eqn:El.
    - apply lvl_eqb_iff in El. destruct Hu as [Hu | Hu]; [subst u; exact Hk |].
      exfalso. exact (proj1 (waiter_head_none _ _) Hlane u Hu El).
    - assert (Hne : w_lane k <> w_lane u).
      { intros E. rewrite E, lvl_eqb_refl in El. discriminate. }
      destruct Hu as [Hu | Hu]; [subst u; rewrite lvl_eqb_refl in El; discriminate |].
      unfold is_waiter_head in Hhu. rewrite (waiter_head_cons_other k _ _ Hne) in Hhu.
      apply Hinv; [exact Hu | exact Hhu].
  Qed.

  (* A datum stored on an empty level that every waiter-lane head that sees
     the level refuses keeps INV. *)
  Lemma store_fresh_level_inv :
    forall st d c, ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) ->
      no_pending_match st -> record_head (s_records st) (r_lvl d) = None ->
      (forall ln u, waiter_head (s_waiters st) ln = Some u -> sees ln (r_lvl d) = true ->
         fits_datum u (r_val d) = false) ->
      no_pending_match (mkS (d :: s_records st) (s_waiters st) c).
  Proof.
    intros st d c Hr Hw Hinv Hlvl Hrefuse u Hu Hhu. cbn [s_records s_waiters] in *.
    assert (Hother : forall y, In y (s_records st) -> r_lvl y <> r_lvl d)
      by exact (proj1 (record_head_none (s_records st) (r_lvl d)) Hlvl).
    assert (Hhead : forall y, In y (s_records st) ->
                      is_record_head (d :: s_records st) y = is_record_head (s_records st) y)
      by (intros y Hy; apply is_record_head_cons_other; exact (Hother y Hy)).
    unfold fireable.
    destruct (first_match (d :: s_records st) (w_lane u) (w_pat u)) as [h |] eqn:Hm; [| reflexivity].
    destruct (first_match_spec _ _ _ _ Hm) as [Hin [Hhh [Hsees [Hsp Hmin]]]].
    destruct Hin as [Hin | Hin].
    - subst h.
      assert (Hu' : waiter_head (s_waiters st) (w_lane u) = Some u)
        by exact (proj1 (is_waiter_head_iff _ u Hw Hu) Hhu).
      pose proof (Hrefuse (w_lane u) u Hu' Hsees) as Hf.
      unfold fits_datum in Hf. rewrite Hsp in Hf. cbn [andb] in Hf. rewrite Hf. reflexivity.
    - rewrite (Hhead h Hin) in Hhh.
      pose proof (Hinv u Hu Hhu) as Hfl. unfold fireable in Hfl.
      destruct (first_match (s_records st) (w_lane u) (w_pat u)) as [h' |] eqn:Hm'.
      + destruct (first_match_spec _ _ _ _ Hm') as [Hin' [Hhh' [Hsees' [Hsp' Hmin']]]].
        assert (Hle1 : lvl_le (r_lvl h') (r_lvl h) = true) by (apply Hmin'; assumption).
        assert (Hle2 : lvl_le (r_lvl h) (r_lvl h') = true).
        { apply Hmin; [right; exact Hin' | rewrite (Hhead h' Hin'); exact Hhh' | exact Hsees'
                      | exact Hsp']. }
        destruct lvl_le_total_order as [Hanti _].
        assert (Heq : h' = h)
          by exact (level_heads_inj _ h' h Hr Hin' Hin Hhh' Hhh (Hanti _ _ Hle1 Hle2)).
        subst h'. exact Hfl.
      + exfalso. rewrite (first_match_none _ _ _ Hm' h Hin Hhh Hsees) in Hsp. discriminate.
  Qed.

  (* Headline 10. A head that does not match blocks its lane (decision 11,
     head-only). A receive whose visible level heads do not match its pattern
     parks and fires nothing. This is true also when a record behind a head of
     its level matches. A send whose candidate heads refuse it is stored and
     fires nothing. This is true also when a waiter behind a head fits it. The
     premises are INV, a fresh lane or level, distinct orders for the send, and
     a cap above 0 (open_plan). *)
  Theorem pq_head_blocks_within_level :
    (forall st rank pat g lane pers peek,
       0 < cap -> no_pending_match st -> waiter_head (s_waiters st) lane = None ->
       first_match (s_records st) lane pat = None ->
       consume st rank pat g lane pers peek =
         mkOutcome [SPark (mkWaiter (bump_ord (s_counter st)) rank lane pat g pers peek)] None) /\
    (forall st rank v lvl pers,
       0 < cap -> ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) ->
       no_pending_match st -> record_head (s_records st) lvl = None ->
       (forall ln u, waiter_head (s_waiters st) ln = Some u -> sees ln lvl = true ->
          fits_datum u v = false) ->
       produce st rank v lvl pers =
         mkOutcome [SStore (mkRecord (bump_ord (s_counter st)) rank lvl v pers)] None).
  Proof.
    split.
    - intros st rank pat g lane pers peek Hcap Hinv Hlane Hm. unfold consume.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + unfold fresh_receive. cbn [bumped s_waiters s_records w_lane w_pat]. rewrite Hlane.
        cbn [receive_loop bumped s_records w_lane w_pat]. rewrite Hm. reflexivity.
      + rewrite park_after_bump. apply park_fresh_lane_inv; [exact Hinv | exact Hlane |].
        unfold fireable. cbn [w_lane w_pat]. rewrite Hm. reflexivity.
    - intros st rank v lvl pers Hcap Hr Hw Hinv Hlvl Hrefuse. unfold produce.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + unfold fresh_offer. cbn [bumped s_records s_waiters r_lvl]. rewrite Hlvl.
        cbn [offer_loop s_waiters].
        destruct (offer_pick (s_waiters st) (mkRecord (bump_ord (s_counter st)) rank lvl v pers)
                    (false, false)) as [[w |] ref'] eqn:Hp;
          [exfalso | exact (offer_loop_store cap 0 (bumped st) _ (false, false) ref' Hp)].
        destruct (proj1 offer_pick_older_first _ _ _ _ _ Hp) as [Hc [Hfit _]].
        unfold candidate, cand1, cand2 in Hc. cbn [fst snd r_lvl r_val] in Hc, Hfit.
        destruct Hc as [Hc | Hc].
        * destruct lvl as [p |]; [| discriminate].
          rewrite (Hrefuse (Some p) w Hc (proj2 (sees_level p (Some p)) eq_refl)) in Hfit.
          discriminate.
        * rewrite (Hrefuse None w Hc (sees_any lvl)) in Hfit. discriminate.
      + rewrite store_after_bump. apply store_fresh_level_inv; [exact Hr | exact Hw | exact Hinv
                                                               | exact Hlvl | exact Hrefuse].
  Qed.

  (* A record stored behind the head of its level keeps every head. *)
  Lemma record_head_cons_behind :
    forall l x h, ords_distinct r_ord (x :: l) -> record_head l (r_lvl x) = Some h ->
      r_ord h < r_ord x -> record_head (x :: l) (r_lvl x) = Some h.
  Proof.
    intros l x h Hnd Hh Hlt. destruct (record_head_spec l (r_lvl x) h Hh) as [Hin [Hl Hmin]].
    unfold record_head. apply least_is_min.
    - apply keys_of_ords. apply ords_distinct_filter. exact Hnd.
    - apply filter_In. split; [right; exact Hin | rewrite Hl; apply lvl_eqb_refl].
    - intros y Hy. apply filter_In in Hy as [[Hy | Hy] Hyl]; apply lvl_eqb_iff in Hyl.
      + subst y. left. exact Hlt.
      + exact (Hmin y Hy Hyl).
  Qed.

  Lemma first_match_cons_behind :
    forall l x h lane pat, ords_distinct r_ord (x :: l) -> record_head l (r_lvl x) = Some h ->
      r_ord h < r_ord x -> first_match (x :: l) lane pat = first_match l lane pat.
  Proof.
    intros l x h lane pat Hnd Hh Hlt.
    pose proof (record_head_cons_behind l x h Hnd Hh Hlt) as Hxh.
    assert (Hheads : forall y, is_record_head (x :: l) y = is_record_head l y).
    { intros y. destruct (lvl_eqb (r_lvl x) (r_lvl y)) eqn:E.
      - apply lvl_eqb_iff in E. unfold is_record_head. rewrite <- E, Hxh, Hh. reflexivity.
      - unfold is_record_head, record_head. cbn [filter]. rewrite E. reflexivity. }
    assert (Hx : is_record_head (x :: l) x = false).
    { unfold is_record_head. rewrite Hxh. unfold rec_key_eqb.
      destruct (r_ord h =? r_ord x) eqn:E; [apply Nat.eqb_eq in E; lia | reflexivity]. }
    unfold first_match. cbn [filter]. rewrite Hx. cbn [andb].
    f_equal. apply filter_ext. intros y. rewrite Hheads. reflexivity.
  Qed.

  Lemma store_behind_inv :
    forall st x h c, ords_distinct r_ord (x :: s_records st) -> no_pending_match st ->
      record_head (s_records st) (r_lvl x) = Some h -> r_ord h < r_ord x ->
      no_pending_match (mkS (x :: s_records st) (s_waiters st) c).
  Proof.
    intros st x h c Hnd Hinv Hh Hlt u Hu Hhu. cbn [s_records s_waiters] in *.
    unfold fireable. rewrite (first_match_cons_behind _ x h _ _ Hnd Hh Hlt).
    exact (Hinv u Hu Hhu).
  Qed.

  Lemma waiter_head_cons_behind :
    forall l k w, ords_distinct w_ord (k :: l) -> waiter_head l (w_lane k) = Some w ->
      w_ord w < w_ord k -> waiter_head (k :: l) (w_lane k) = Some w.
  Proof.
    intros l k w Hnd Hw Hlt. destruct (waiter_head_spec l (w_lane k) w Hw) as [Hin [Hl Hmin]].
    unfold waiter_head. apply least_is_min.
    - apply keys_of_ords. apply ords_distinct_filter. exact Hnd.
    - apply filter_In. split; [right; exact Hin | rewrite Hl; apply lvl_eqb_refl].
    - intros y Hy. apply filter_In in Hy as [[Hy | Hy] Hyl]; apply lvl_eqb_iff in Hyl.
      + subst y. left. exact Hlt.
      + exact (Hmin y Hy Hyl).
  Qed.

  Lemma park_behind_inv :
    forall st k w c, ords_distinct w_ord (k :: s_waiters st) -> no_pending_match st ->
      waiter_head (s_waiters st) (w_lane k) = Some w -> w_ord w < w_ord k ->
      no_pending_match (mkS (s_records st) (k :: s_waiters st) c).
  Proof.
    intros st k w c Hnd Hinv Hw Hlt u Hu Hhu. cbn [s_records s_waiters] in *.
    pose proof (waiter_head_cons_behind _ k w Hnd Hw Hlt) as Hkw.
    destruct Hu as [Hu | Hu].
    - subst u. exfalso. unfold is_waiter_head in Hhu. rewrite Hkw in Hhu. unfold wtr_key_eqb in Hhu.
      destruct (w_ord w =? w_ord k) eqn:E; [apply Nat.eqb_eq in E; lia | discriminate].
    - assert (Hsame : is_waiter_head (k :: s_waiters st) u = is_waiter_head (s_waiters st) u).
      { destruct (lvl_eqb (w_lane k) (w_lane u)) eqn:E.
        - apply lvl_eqb_iff in E. unfold is_waiter_head. rewrite <- E, Hkw, Hw. reflexivity.
        - unfold is_waiter_head, waiter_head. cbn [filter]. rewrite E. reflexivity. }
      rewrite Hsame in Hhu. exact (Hinv u Hu Hhu).
  Qed.

  (* A re-entry takes its FIFO place (decision 43). A re-issued persistent
     element can have a first-round order that follows the order of the older
     head of its lane. Then the element is stored or parked behind that head.
     It fires nothing, also when a head of the other side fits it. An element
     ahead of the head of its lane, or on an empty lane, plans as a fresh
     element at the head of its lane. The plan starts from the fire count f of
     its transaction. The premises are INV, distinct orders, the element
     included (wf' of decision 43), and a cap above 0 (open_plan). *)
  Lemma reentry_at_fifo_place :
    (forall st x h,
       0 < cap -> no_pending_match st -> ords_distinct r_ord (x :: s_records st) ->
       record_head (s_records st) (r_lvl x) = Some h -> r_ord h < r_ord x ->
       reenter_produce st x = mkOutcome [SStore x] None) /\
    (forall st k w,
       0 < cap -> no_pending_match st -> ords_distinct w_ord (k :: s_waiters st) ->
       waiter_head (s_waiters st) (w_lane k) = Some w -> w_ord w < w_ord k ->
       reenter_consume st k = mkOutcome [SPark k] None) /\
    (forall f s x,
       match record_head (s_records s) (r_lvl x) with Some h => r_ord x < r_ord h | None => True end ->
       reenter_offer x f s = offer_loop (S cap) f s x (false, false)) /\
    (forall f s k,
       match waiter_head (s_waiters s) (w_lane k) with Some w => w_ord k < w_ord w | None => True end ->
       reenter_receive k f s = receive_loop (S cap) f s k).
  Proof.
    split; [| split; [| split]].
    - intros st x h Hcap Hinv Hnd Hh Hlt. unfold reenter_produce.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + unfold reenter_offer. cbn [bumped s_records]. rewrite Hh.
        destruct (r_ord x <? r_ord h) eqn:E; [apply Nat.ltb_lt in E; lia | reflexivity].
      + rewrite store_after_bump. exact (store_behind_inv st x h _ Hnd Hinv Hh Hlt).
    - intros st k w Hcap Hinv Hnd Hw Hlt. unfold reenter_consume.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + unfold reenter_receive. cbn [bumped s_waiters]. rewrite Hw.
        destruct (w_ord k <? w_ord w) eqn:E; [apply Nat.ltb_lt in E; lia | reflexivity].
      + rewrite park_after_bump. exact (park_behind_inv st k w _ Hnd Hinv Hw Hlt).
    - intros f s x H. unfold reenter_offer.
      destruct (record_head (s_records s) (r_lvl x)) as [h |]; [| reflexivity].
      destruct (r_ord x <? r_ord h) eqn:E; [reflexivity | apply Nat.ltb_ge in E; lia].
    - intros f s k H. unfold reenter_receive.
      destruct (waiter_head (s_waiters s) (w_lane k)) as [w |]; [| reflexivity].
      destruct (w_ord k <? w_ord w) eqn:E; [reflexivity | apply Nat.ltb_ge in E; lia].
  Qed.

  (* Headline 18. A peek restores the head (decision 45, Rule E). A peek of a
     level head that is not persistent fires and removes the record. The peek
     stores the record again at its order and level before any settle step. The
     record is then the head of its level again, and the heads are the heads
     before the peek. So the settle after the plan stages nothing. The records
     are a permutation of the records before, the waiters are unchanged, and
     INV holds. The receive meets an empty lane, because a fresh continuation
     joins the tail of its lane. The cap is above 0, so open_plan opens the
     plan. *)
  Theorem pq_peek_restores_head :
    forall st rank pat g lane h,
      0 < cap -> ords_distinct r_ord (s_records st) -> no_pending_match st ->
      waiter_head (s_waiters st) lane = None ->
      first_match (s_records st) lane pat = Some h -> admits g (r_val h) = true ->
      r_pers h = false ->
      let k := mkWaiter (bump_ord (s_counter st)) rank lane pat g false true in
      consume st rank pat g lane false true = mkOutcome [SFireConsume k h; SRestore h] None /\
      record_head (s_records (after st (consume st rank pat g lane false true))) (r_lvl h) = Some h /\
      Permutation (s_records (after st (consume st rank pat g lane false true))) (s_records st) /\
      s_waiters (after st (consume st rank pat g lane false true)) = s_waiters st /\
      no_pending_match (after st (consume st rank pat g lane false true)).
  Proof.
    intros st rank pat g lane h Hcap Hnd Hinv Hlane Hm Ha Hpers k.
    destruct (first_match_spec _ _ _ _ Hm) as [Hin [Hhead _]].
    assert (Hrun : run (bumped st) [SFireConsume k h; SRestore h] =
                   mkS (h :: drop_record h (s_records st)) (s_waiters st) (bump_next (s_counter st))).
    { cbn [run fold_left apply bumped s_records s_waiters s_counter]. rewrite Hpers. reflexivity. }
    assert (Hperm : Permutation (h :: drop_record h (s_records st)) (s_records st))
      by (apply drop_record_perm; assumption).
    assert (Hafter : no_pending_match (run (bumped st) [SFireConsume k h; SRestore h])).
    { rewrite Hrun. intros u Hu Hhu. cbn [s_records s_waiters] in *.
      rewrite (fireable_perm _ (s_records st) u Hperm).
      - exact (Hinv u Hu Hhu).
      - apply (ords_distinct_perm record r_ord (s_records st)); [apply Permutation_sym; exact Hperm
                                                                | exact Hnd]. }
    assert (Hout : consume st rank pat g lane false true = mkOutcome [SFireConsume k h; SRestore h] None).
    { unfold consume. apply with_settles_quiet; [exact Hcap | exact Hinv | | exact Hafter].
      unfold fresh_receive. cbn [bumped s_waiters s_records w_lane w_pat w_guard]. rewrite Hlane.
      cbn [receive_loop bumped s_records w_lane w_pat w_guard]. rewrite Hm, Ha.
      unfold consume_fire. cbn [w_peek w_pers]. rewrite Hpers. cbn [andb negb orb].
      unfold after_fire. cbn [negb]. reflexivity. }
    split; [exact Hout |]. unfold after. rewrite Hout. cbn [o_ops].
    split.
    { rewrite Hrun. cbn [s_records].
      rewrite (record_head_perm _ (s_records st) (r_lvl h) Hperm).
      - exact (proj1 (is_record_head_iff _ h Hnd Hin) Hhead).
      - apply (ords_distinct_perm record r_ord (s_records st)); [apply Permutation_sym; exact Hperm
                                                                | exact Hnd]. }
    split; [rewrite Hrun; exact Hperm |].
    split; [rewrite Hrun; reflexivity | exact Hafter].
  Qed.

  (* ── 14. The re-issue keeps the first-round order (decision 43) ──────── *)

  Lemma with_settles_carried :
    forall st pers o plan c ca,
      o_next (with_settles st pers o plan) = Some (NextOperation c ca) -> ca = carried pers o.
  Proof.
    intros st pers o plan c ca H. unfold with_settles in H.
    destruct (z_end (settle_of st)) as [| c0]; cbn [o_next] in H.
    - destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_next] in H;
        [injection H as _ <-; reflexivity |].
      match type of H with context [p_end ?p] => destruct (p_end p) as [| c1] end;
        cbn [o_next] in H.
      + exfalso. exact (settle_next_operation _ _ _ H).
      + injection H as _ <-. reflexivity.
    - injection H as _ <-. reflexivity.
  Qed.

  Lemma apply_counter : forall st op, s_counter (apply st op) = s_counter st.
  Proof.
    intros st op. destruct op as [r | w | w r | k h | h | r]; cbn [apply];
      try reflexivity; [destruct (w_pers w) | destruct (r_pers h)]; reflexivity.
  Qed.

  Lemma run_counter : forall ops st, s_counter (run st ops) = s_counter st.
  Proof.
    induction ops as [| op rest IH]; intros st; [reflexivity |].
    rewrite run_cons, IH. apply apply_counter.
  Qed.

  (* A settle stages no store of a datum: it lifts, fires and stores again
     only records of the view. *)
  Lemma settle_no_store :
    forall fuel fired st x r, In (SStore r) (z_ops (settle_loop fuel fired st x)) -> False.
  Proof.
    assert (Hland : forall x r, In (SStore r) (land x) -> False).
    { intros [r0 |] r H; cbn [land In] in H; [destruct H as [H | []]; discriminate | exact H]. }
    induction fuel as [| fuel IH]; intros fired st x r H; cbn [settle_loop] in H.
    - exact (Hland x r H).
    - destruct (next_fire st x) as [[[w r0] b] |]; [| exact (Hland x r H)].
      destruct (lands_first x b).
      + cbn [z_ops] in H. apply in_app_or in H as [H | H]; [exact (Hland _ _ H) | exact (IH _ _ _ _ H)].
      + destruct (at_cap fired); [exact (Hland x r H) |].
        assert (Hstep : In (SStore r) (lift_ops x r0 ++ [SFireProduce w r0]) -> False).
        { unfold lift_ops. destruct x; [| destruct (r_pers r0)]; cbn [app In];
            intros Hin; repeat (destruct Hin as [Hin | Hin]; [discriminate |]); exact Hin. }
        destruct (after_fire (S fired) (negb (r_pers r0) && negb (w_peek w)) (w_pers w));
          cbn [z_ops] in H; apply in_app_or in H as [H | H];
          solve [exact (Hstep H) | exact (IH _ _ _ _ H) | exact (Hland _ _ H)].
  Qed.

  (* The offer loop stores only its own datum. *)
  Lemma offer_loop_stores :
    forall fuel fired st r refused r',
      In (SStore r') (p_ops (offer_loop fuel fired st r refused)) -> r' = r.
  Proof.
    induction fuel as [| fuel IH]; intros fired st r refused r' H; cbn [offer_loop] in H; [destruct H |].
    destruct (offer_pick (s_waiters st) r refused) as [[w |] refused'];
      [| destruct H as [H | []]; injection H as <-; reflexivity].
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [p_ops In] in H; destruct H as [H | H]; try discriminate; try destruct H.
    exact (IH _ _ _ _ _ H).
  Qed.

  (* An operation stores only what its plan stores, from some fire count. *)
  Lemma with_settles_stores :
    forall st pers o plan r,
      In (SStore r) (o_ops (with_settles st pers o plan)) ->
      exists f, In (SStore r) (p_ops (plan f (run st (z_ops (settle_of st))))).
  Proof.
    intros st pers o plan r H. unfold with_settles in H.
    destruct (z_end (settle_of st)); cbn [o_ops] in H; [| exfalso; exact (settle_no_store _ _ _ _ _ H)].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_ops] in H;
      [exfalso; exact (settle_no_store _ _ _ _ _ H) |].
    match type of H with context [p_end ?p] => destruct (p_end p) end; cbn [o_ops] in H;
      apply in_app_or in H as [H | H]; try (exfalso; exact (settle_no_store _ _ _ _ _ H));
      try (eexists; exact H).
    apply in_app_or in H as [H | H]; [eexists; exact H | exfalso; exact (settle_no_store _ _ _ _ _ H)].
  Qed.

  (* A settle stages no park: it lifts, fires and stores again only records
     of the view. *)
  Lemma settle_no_park :
    forall fuel fired st x k, In (SPark k) (z_ops (settle_loop fuel fired st x)) -> False.
  Proof.
    assert (Hland : forall x k, In (SPark k) (land x) -> False).
    { intros [r0 |] k H; cbn [land In] in H; [destruct H as [H | []]; discriminate | exact H]. }
    induction fuel as [| fuel IH]; intros fired st x k H; cbn [settle_loop] in H.
    - exact (Hland x k H).
    - destruct (next_fire st x) as [[[w r0] b] |]; [| exact (Hland x k H)].
      destruct (lands_first x b).
      + cbn [z_ops] in H. apply in_app_or in H as [H | H]; [exact (Hland _ _ H) | exact (IH _ _ _ _ H)].
      + destruct (at_cap fired); [exact (Hland x k H) |].
        assert (Hstep : In (SPark k) (lift_ops x r0 ++ [SFireProduce w r0]) -> False).
        { unfold lift_ops. destruct x; [| destruct (r_pers r0)]; cbn [app In];
            intros Hin; repeat (destruct Hin as [Hin | Hin]; [discriminate |]); exact Hin. }
        destruct (after_fire (S fired) (negb (r_pers r0) && negb (w_peek w)) (w_pers w));
          cbn [z_ops] in H; apply in_app_or in H as [H | H];
          solve [exact (Hstep H) | exact (IH _ _ _ _ H) | exact (Hland _ _ H)].
  Qed.

  (* The receive loop parks only its own continuation. *)
  Lemma receive_loop_parks :
    forall fuel fired st k k', In (SPark k') (p_ops (receive_loop fuel fired st k)) -> k' = k.
  Proof.
    induction fuel as [| fuel IH]; intros fired st k k' H; cbn [receive_loop] in H; [destruct H |].
    destruct (first_match (s_records st) (w_lane k) (w_pat k)) as [h |];
      [| destruct H as [H | []]; injection H as <-; reflexivity].
    destruct (admits (w_guard k) (r_val h)); [| destruct H as [H | []]; injection H as <-; reflexivity].
    assert (Hfire : In (SPark k') (consume_fire k h) -> False).
    { unfold consume_fire. destruct (w_peek k && negb (r_pers h)); cbn [In];
        intros Hin; repeat (destruct Hin as [Hin | Hin]; [discriminate |]); exact Hin. }
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)); cbn [p_ops] in H;
      try (exfalso; exact (Hfire H)).
    apply in_app_or in H as [H | H]; [exfalso; exact (Hfire H) | exact (IH _ _ _ _ H)].
  Qed.

  (* An operation parks only what its plan parks, from some fire count. *)
  Lemma with_settles_parks :
    forall st pers o plan k,
      In (SPark k) (o_ops (with_settles st pers o plan)) ->
      exists f, In (SPark k) (p_ops (plan f (run st (z_ops (settle_of st))))).
  Proof.
    intros st pers o plan k H. unfold with_settles in H.
    destruct (z_end (settle_of st)); cbn [o_ops] in H; [| exfalso; exact (settle_no_park _ _ _ _ _ H)].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_ops] in H;
      [exfalso; exact (settle_no_park _ _ _ _ _ H) |].
    match type of H with context [p_end ?p] => destruct (p_end p) end; cbn [o_ops] in H;
      apply in_app_or in H as [H | H]; try (exfalso; exact (settle_no_park _ _ _ _ _ H));
      try (eexists; exact H).
    apply in_app_or in H as [H | H]; [eexists; exact H | exfalso; exact (settle_no_park _ _ _ _ _ H)].
  Qed.

  (* Headline 16. A re-issued send takes its FIFO place in its level (decision
     43). A re-issued persistent datum can have a first-round order that
     follows the order of the older head of its level. Then the datum is stored
     behind that head, with no fire. A datum ahead of the head of its level, or
     on an empty level, plans as a fresh datum at the head of its level. A
     re-issued send carries the order of its first round exactly when it is
     persistent. A fresh send carries the order of its bump. A re-entry carries
     its first-round order again. Every round bumps the one counter of the
     priority queue once. A re-entered datum stores only itself, so its record,
     its level and its identity stay the same in every round. The premises of
     the store are INV, distinct orders, the datum included (wf' of decision
     43), and a cap above 0 (open_plan). *)
  Theorem pq_reissue_takes_level_fifo_place :
    (forall st x h,
       0 < cap -> no_pending_match st -> ords_distinct r_ord (x :: s_records st) ->
       record_head (s_records st) (r_lvl x) = Some h -> r_ord h < r_ord x ->
       reenter_produce st x = mkOutcome [SStore x] None) /\
    (forall f s x,
       match record_head (s_records s) (r_lvl x) with Some h => r_ord x < r_ord h | None => True end ->
       reenter_offer x f s = offer_loop (S cap) f s x (false, false)) /\
    (forall st rank v lvl pers c ca,
       o_next (produce st rank v lvl pers) = Some (NextOperation c ca) ->
       ca = carried pers (bump_ord (s_counter st))) /\
    (forall st x c ca,
       r_pers x = true -> o_next (reenter_produce st x) = Some (NextOperation c ca) ->
       ca = Some (r_ord x)) /\
    (forall st o, s_counter (after st o) = Some (S (bump_ord (s_counter st)))) /\
    (forall st x r, In (SStore r) (o_ops (reenter_produce st x)) -> r = x).
  Proof.
    destruct reentry_at_fifo_place as [Hstore [_ [Hplan _]]].
    split; [exact Hstore | split; [exact Hplan |]].
    split; [| split; [| split]].
    - intros st rank v lvl pers c ca H. exact (with_settles_carried _ _ _ _ _ _ H).
    - intros st x c ca Hp H. rewrite (with_settles_carried _ _ _ _ _ _ H). unfold carried.
      rewrite Hp. reflexivity.
    - intros st o. unfold after. rewrite run_counter. reflexivity.
    - intros st x r H. unfold reenter_produce in H. apply with_settles_stores in H.
      destruct H as [f H]. unfold reenter_offer in H.
      destruct (record_head (s_records (run (bumped st) (z_ops (settle_of (bumped st))))) (r_lvl x))
        as [h |]; [destruct (r_ord x <? r_ord h) |];
        solve [exact (offer_loop_stores _ _ _ _ _ _ H)
              | destruct H as [H | []]; injection H as <-; reflexivity].
  Qed.

  (* Headline 17. A re-entered waiter keeps its seniority (decision 43). A
     re-issued persistent continuation can have a first-round order that
     follows the order of the older head of its waiter queue. Then the
     continuation parks behind that head, with no fire. A continuation ahead of
     the head of its queue, or on an empty queue, plans as a fresh continuation
     at the head of its queue. A re-issued receive carries the order of its
     first round exactly when it is persistent. A re-entered receive parks only
     itself. So the parked waiter keeps its first-round order, and the rule
     "the older waiter fires first" (headline 6) ranks it with that order. The
     premises of the park are INV, distinct orders, the continuation included
     (wf' of decision 43), and a cap above 0 (open_plan). *)
  Theorem pq_reentered_waiter_keeps_seniority :
    (forall st k w,
       0 < cap -> no_pending_match st -> ords_distinct w_ord (k :: s_waiters st) ->
       waiter_head (s_waiters st) (w_lane k) = Some w -> w_ord w < w_ord k ->
       reenter_consume st k = mkOutcome [SPark k] None) /\
    (forall f s k,
       match waiter_head (s_waiters s) (w_lane k) with Some w => w_ord k < w_ord w | None => True end ->
       reenter_receive k f s = receive_loop (S cap) f s k) /\
    (forall st rank pat g lane pers peek c ca,
       o_next (consume st rank pat g lane pers peek) = Some (NextOperation c ca) ->
       ca = carried pers (bump_ord (s_counter st))) /\
    (forall st k c ca,
       w_pers k = true -> o_next (reenter_consume st k) = Some (NextOperation c ca) ->
       ca = Some (w_ord k)) /\
    (forall st k k', In (SPark k') (o_ops (reenter_consume st k)) -> k' = k).
  Proof.
    destruct reentry_at_fifo_place as [_ [Hpark [_ Hplan]]].
    split; [exact Hpark | split; [exact Hplan |]].
    split; [| split].
    - intros st rank pat g lane pers peek c ca H. exact (with_settles_carried _ _ _ _ _ _ H).
    - intros st k c ca Hp H. rewrite (with_settles_carried _ _ _ _ _ _ H). unfold carried.
      rewrite Hp. reflexivity.
    - intros st k k' H. unfold reenter_consume in H. apply with_settles_parks in H.
      destruct H as [f H]. unfold reenter_receive in H.
      destruct (waiter_head (s_waiters (run (bumped st) (z_ops (settle_of (bumped st))))) (w_lane k))
        as [w |]; [destruct (w_ord k <? w_ord w) |];
        solve [exact (receive_loop_parks _ _ _ _ _ H)
              | destruct H as [H | []]; injection H as <-; reflexivity].
  Qed.

  (* ── 15. A settle loses no record ─────────────────────────────────── *)

  Lemma drop_record_keeps :
    forall l r h, ords_distinct r_ord l -> In h l -> In r l -> h <> r -> In h (drop_record r l).
  Proof.
    intros l r h Hnd Hh Hr Hne. unfold drop_record.
    revert Hnd Hh Hr. induction l as [| y rest IH]; intros Hnd Hh Hr; [destruct Hh |].
    pose proof Hnd as Hnd0. unfold ords_distinct in Hnd. cbn [map] in Hnd.
    apply NoDup_cons_iff in Hnd as [_ Hnd]. cbn [drop_key].
    destruct ((r_ord y =? r_ord r) && (r_rank y =? r_rank r)) eqn:Ey.
    - apply andb_true_iff in Ey as [Eo _]. apply Nat.eqb_eq in Eo.
      assert (Hy : y = r)
        by (apply (ords_inj record r_ord (y :: rest) y r Hnd0); [left; reflexivity | exact Hr | exact Eo]).
      subst y. destruct Hh as [Hh | Hh]; [subst; contradiction | exact Hh].
    - destruct Hh as [Hh | Hh]; [left; exact Hh |].
      destruct Hr as [Hr | Hr]; [subst y; rewrite !Nat.eqb_refl in Ey; discriminate |].
      right. apply IH; assumption.
  Qed.

  Lemma land_keeps :
    forall st x h, In h (s_records st) \/ x = Some h -> In h (s_records (run st (land x))).
  Proof.
    intros st [r |] h [Hh | Hh]; cbn [land run fold_left apply s_records].
    - right. exact Hh.
    - injection Hh as <-. left. reflexivity.
    - exact Hh.
    - discriminate.
  Qed.

  (* The loop form: a record that is stored or in flight stays, or a fire of
     a waiter that does not peek spends it. A landing before a fire of a
     stored record stores the record in flight again. *)
  Lemma settle_loop_keeps :
    forall fuel fired st x,
      settle_inv st x ->
      forall h, In h (s_records st) \/ x = Some h ->
        In h (s_records (run st (z_ops (settle_loop fuel fired st x)))) \/
        (r_pers h = false /\
         exists w, In (SFireProduce w h) (z_ops (settle_loop fuel fired st x)) /\ w_peek w = false).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x Hinv h Hh; cbn [settle_loop].
    - left. apply land_keeps. exact Hh.
    - destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf; [| left; apply land_keeps; exact Hh].
      destruct (lands_first x b) eqn:Hl.
      + cbn [z_ops].
        destruct (IH fired (run st (land x)) None (land_inv st x Hinv) h
                    (or_introl (land_keeps st x h Hh))) as [Hk | [Hp [w1 [Hin1 Hk1]]]].
        * left. rewrite run_app. exact Hk.
        * right. split; [exact Hp |]. exists w1.
          split; [apply in_or_app; right; exact Hin1 | exact Hk1].
      + destruct (at_cap fired); [left; apply land_keeps; exact Hh |].
        pose proof (next_fire_bound st x w r b Hinv Hnf Hl) as Hrx.
        set (spent := negb (r_pers r) && negb (w_peek w)).
        set (ops := lift_ops x r ++ [SFireProduce w r]).
        assert (Hinv1 : settle_inv (run st ops) (flight_after x r spent))
          by (apply (settle_step_inv st x w r b); assumption).
        assert (Hfire : In (SFireProduce w r) ops)
          by (unfold ops; apply in_or_app; right; left; reflexivity).
        assert (Hstep : In h (s_records (run st ops)) \/ flight_after x r spent = Some h \/
                        (r_pers h = false /\ In (SFireProduce w h) ops /\ w_peek w = false)).
        { destruct Hinv as [Hr [_ Hx]].
          unfold ops. rewrite settle_step_records.
          destruct x as [r0 |].
          - subst r0. destruct Hx as [Hp _].
            destruct Hh as [Hh | Hh]; [left; exact Hh |]. injection Hh as <-.
            unfold flight_after, flying, spent. rewrite Hp. cbn [negb andb].
            destruct (w_peek w) eqn:Ek; cbn [negb].
            + right. left. reflexivity.
            + right. right. split; [reflexivity | split; [exact Hfire | reflexivity]].
          - destruct Hh as [Hh | Hh]; [| discriminate].
            destruct (r_pers r) eqn:Ep; [left; exact Hh |].
            destruct (Nat.eq_dec (r_ord h) (r_ord r)) as [Eo | Ene].
            + assert (Heq : h = r) by exact (ords_inj record r_ord (s_records st) h r Hr Hh Hrx Eo).
              subst h. unfold flight_after, flying, spent. rewrite Ep. cbn [negb andb].
              destruct (w_peek w) eqn:Ek; cbn [negb].
              * right. left. reflexivity.
              * right. right. split; [reflexivity | split; [exact Hfire | reflexivity]].
            + left. apply drop_record_keeps; [exact Hr | exact Hh | exact Hrx |].
              intros Heq. subst h. contradiction. }
        destruct (after_fire (S fired) spent (w_pers w)); cbn [z_ops].
        * destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
          -- destruct (IH (S fired) _ _ Hinv1 h (or_introl Hs)) as [Hk | [Hp [w1 [Hin1 Hk1]]]];
               [left; rewrite run_app; exact Hk |].
             right. split; [exact Hp |]. exists w1.
             split; [apply in_or_app; right; exact Hin1 | exact Hk1].
          -- destruct (IH (S fired) _ _ Hinv1 h (or_intror Hs)) as [Hk | [Hp [w1 [Hin1 Hk1]]]];
               [left; rewrite run_app; exact Hk |].
             right. split; [exact Hp |]. exists w1.
             split; [apply in_or_app; right; exact Hin1 | exact Hk1].
          -- right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
        * destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
          -- destruct (IH (S fired) _ _ Hinv1 h (or_introl Hs)) as [Hk | [Hp [w1 [Hin1 Hk1]]]];
               [left; rewrite run_app; exact Hk |].
             right. split; [exact Hp |]. exists w1.
             split; [apply in_or_app; right; exact Hin1 | exact Hk1].
          -- destruct (IH (S fired) _ _ Hinv1 h (or_intror Hs)) as [Hk | [Hp [w1 [Hin1 Hk1]]]];
               [left; rewrite run_app; exact Hk |].
             right. split; [exact Hp |]. exists w1.
             split; [apply in_or_app; right; exact Hin1 | exact Hk1].
          -- right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
        * destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
          -- left. rewrite run_app. apply land_keeps. left. exact Hs.
          -- left. rewrite run_app. apply land_keeps. right. exact Hs.
          -- right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
        * destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
          -- left. rewrite run_app. apply land_keeps. left. exact Hs.
          -- left. rewrite run_app. apply land_keeps. right. exact Hs.
          -- right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
  Qed.

  (* Headline 20. A settle loses no record (decisions 30 and 45,
     PqNoRecordLost). A stored record leaves the store during a settle only
     when a fire of a waiter that does not peek spends it. Then the record is
     not persistent. A lifted record whose fire a waiter that peeks did not
     spend stays in flight. The settle stores it again with its bytes before a
     fire of a stored record. It also stores it again when it has no further
     fire and at a re-issue. This holds from every fire count of the
     transaction (I-21). The premise is wf: distinct orders on each side. *)
  Theorem pq_settle_keeps_unspent_records :
    forall fired st,
      ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) ->
      forall h, In h (s_records st) ->
        In h (s_records (run st (z_ops (settle_at fired st)))) \/
        (r_pers h = false /\
         exists w, In (SFireProduce w h) (z_ops (settle_at fired st)) /\ w_peek w = false).
  Proof.
    intros fired st Hr Hw h Hh. unfold settle_at.
    apply settle_loop_keeps; [split; [exact Hr | split; [exact Hw | exact I]] | left; exact Hh].
  Qed.

  (* ── 16. Termination of the settle ───────────────────────────────────── *)

  Lemma drop_key_count :
    forall (E : Type) (ord rank : E -> nat) (pers : E -> bool) l h,
      ords_distinct ord l -> In h l ->
      length (filter (fun x => negb (pers x)) (drop_key ord rank (ord h) (rank h) l))
        + (if pers h then 0 else 1)
      = length (filter (fun x => negb (pers x)) l).
  Proof.
    intros E ord rank pers l. induction l as [| y rest IH]; intros h Hnd Hin; [destruct Hin |].
    cbn [drop_key]. pose proof Hnd as Hnd0. unfold ords_distinct in Hnd. cbn [map] in Hnd.
    apply NoDup_cons_iff in Hnd as [_ Hnd].
    destruct ((ord y =? ord h) && (rank y =? rank h)) eqn:Ek.
    - apply andb_true_iff in Ek as [Eo _]. apply Nat.eqb_eq in Eo.
      assert (Hy : y = h)
        by (apply (ords_inj E ord (y :: rest) y h Hnd0); [left; reflexivity | exact Hin | exact Eo]).
      subst y. cbn [filter]. destruct (pers h); cbn [negb length]; lia.
    - destruct Hin as [Hin | Hin]; [subst y; rewrite !Nat.eqb_refl in Ek; discriminate |].
      cbn [filter]. specialize (IH h Hnd Hin). destruct (pers y); cbn [negb length]; lia.
  Qed.

  (* μ of the termination argument: the stored records and waiters that are
     not persistent. *)
  Definition np_records (l : list record) : nat := length (filter (fun r => negb (r_pers r)) l).
  Definition np_waiters (l : list waiter) : nat := length (filter (fun w => negb (w_pers w)) l).
  Definition measure (st : sstate) : nat := np_records (s_records st) + np_waiters (s_waiters st).

  (* The record in flight counts once. *)
  Definition flight_measure (x : option record) : nat := match x with Some _ => 1 | None => 0 end.

  (* A divergent pair ends a settle with a fire that lowers nothing. *)
  Definition divergent_end (e : settle_end) : nat :=
    match e with SettleReissue ReissueDivergent => 1 | _ => 0 end.

  Lemma land_measure :
    forall st x, settle_inv st x -> measure (run st (land x)) = measure st + flight_measure x.
  Proof.
    intros st [r |] [_ [_ Hx]]; cbn [land run fold_left apply flight_measure]; [| lia].
    destruct Hx as [Hp _]. unfold measure, np_records. cbn [s_records s_waiters filter].
    rewrite Hp. cbn [negb length]. lia.
  Qed.

  Lemma drop_record_count :
    forall l h, ords_distinct r_ord l -> In h l ->
      np_records (drop_record h l) + (if r_pers h then 0 else 1) = np_records l.
  Proof. intros l h Hnd Hin. exact (drop_key_count record r_ord r_rank r_pers l h Hnd Hin). Qed.

  Lemma drop_waiter_count :
    forall l w, ords_distinct w_ord l -> In w l ->
      np_waiters (drop_waiter w l) + (if w_pers w then 0 else 1) = np_waiters l.
  Proof. intros l w Hnd Hin. exact (drop_key_count waiter w_ord w_rank w_pers l w Hnd Hin). Qed.

  Lemma flight_measure_after :
    forall x r spent,
      flight_measure (flight_after x r spent) <= if flying x r && negb spent then 1 else 0.
  Proof.
    intros x r spent. destruct (flight_after x r spent) as [r1 |] eqn:Ef; cbn [flight_measure]; [| lia].
    destruct (flight_after_some x r spent r1 Ef) as [_ [Hfly Hsp]]. rewrite Hfly, Hsp.
    cbn [andb negb]. lia.
  Qed.

  Lemma flight_measure_le : forall x, flight_measure x <= 1.
  Proof. intros [r |]; cbn; lia. Qed.

  (* A landing before a fire of a stored record needs a record in flight. *)
  Lemma lands_first_flight : forall x b, lands_first x b = true -> flight_measure x = 1.
  Proof. intros [r |] b H; [reflexivity | discriminate]. Qed.

  (* A fire of the settle lowers μ (the record in flight counted), unless it
     is a divergent pair, which keeps it. *)
  Lemma settle_step_measure :
    forall st x w r b,
      settle_inv st x -> next_fire st x = Some (w, r, b) -> lands_first x b = false ->
      let spent := negb (r_pers r) && negb (w_peek w) in
      measure (run st (lift_ops x r ++ [SFireProduce w r])) + flight_measure (flight_after x r spent)
        + (if negb spent && w_pers w then 0 else 1)
      <= measure st + flight_measure x.
  Proof.
    intros st x w r b Hinv Hnf Hl spent.
    pose proof (next_fire_bound st x w r b Hinv Hnf Hl) as Hrx.
    destruct Hinv as [Hr [Hw Hx]].
    destruct (next_fire_some st x w r b Hnf) as [Hwin _].
    pose proof (drop_waiter_count (s_waiters st) w Hw Hwin) as Cw.
    pose proof (flight_measure_after x r spent) as Hfm.
    unfold measure. rewrite settle_step_records, settle_step_waiters.
    unfold spent, flying in *.
    destruct x as [r0 |].
    - subst r0. destruct Hx as [Hp _]. rewrite Hp in *. revert Cw Hfm.
      destruct (w_pers w), (w_peek w); cbn [negb andb flight_measure]; intros Cw Hfm; lia.
    - pose proof (drop_record_count (s_records st) r Hr Hrx) as Cr.
      revert Cw Cr Hfm.
      destruct (r_pers r), (w_pers w), (w_peek w); cbn [negb andb flight_measure];
        intros Cw Cr Hfm; lia.
  Qed.

  (* The bounds of the settle loop. A fuel above 2 * (cap - fired) plus the
     record in flight is enough. Each fire takes one round. Each landing before
     a fire of a stored record takes one round and leaves nothing in flight. *)
  Lemma settle_loop_bounds :
    forall fuel fired st x,
      settle_inv st x -> fired <= cap -> 2 * (cap - fired) + flight_measure x < fuel ->
      let z := settle_loop fuel fired st x in
      fired + fires (z_ops z) <= cap /\
      fires (z_ops z) + measure (run st (z_ops z))
        <= measure st + flight_measure x + divergent_end (z_end z) /\
      (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap) /\
      (z_end z = SettleReissue ReissueDivergent ->
         exists w r, In (SFireProduce w r) (z_ops z) /\ w_pers w = true).
  Proof.
    assert (Hland0 : forall x, fires (land x) = 0) by (intros [r |]; reflexivity).
    induction fuel as [| fuel IH]; intros fired st x Hinv Hle Hfuel z.
    - exfalso. lia.
    - subst z. cbn [settle_loop].
      destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf.
      2: { cbn [z_ops z_end divergent_end]. rewrite Hland0, (land_measure st x Hinv).
           split; [lia | split; [lia | split; intros H; discriminate]]. }
      destruct (lands_first x b) eqn:Hl.
      { pose proof (lands_first_flight x b Hl) as Hx.
        specialize (IH fired (run st (land x)) None (land_inv st x Hinv) Hle
                      ltac:(cbn [flight_measure]; lia)).
        cbn zeta in IH. destruct IH as [I1 [I2 [I3 I4]]].
        rewrite (land_measure st x Hinv) in I2. cbn [flight_measure] in I2.
        cbn [z_ops z_end]. rewrite fires_app, Hland0, run_app.
        split; [lia | split; [lia | split]].
        - intros H. specialize (I3 H). lia.
        - intros H. destruct (I4 H) as [w1 [r1 [Hin1 Hp1]]]. exists w1, r1.
          split; [apply in_or_app; right; exact Hin1 | exact Hp1]. }
      destruct (at_cap fired) eqn:Hcap.
      { unfold at_cap in Hcap. apply Nat.leb_le in Hcap. cbn [z_ops z_end divergent_end].
        rewrite Hland0, (land_measure st x Hinv).
        split; [lia | split; [lia | split; [intros _; lia | intros H; discriminate]]]. }
      unfold at_cap in Hcap. apply Nat.leb_gt in Hcap.
      pose proof (settle_step_measure st x w r b Hinv Hnf Hl) as Hm. cbn zeta in Hm.
      set (spent := negb (r_pers r) && negb (w_peek w)) in *.
      set (ops := lift_ops x r ++ [SFireProduce w r]) in *.
      assert (Hops : fires ops = 1).
      { unfold ops, lift_ops. destruct x; [| destruct (r_pers r)]; reflexivity. }
      assert (Hinv1 : settle_inv (run st ops) (flight_after x r spent))
        by (apply (settle_step_inv st x w r b); assumption).
      assert (Hin : In (SFireProduce w r) ops) by (unfold ops; apply in_or_app; right; left; reflexivity).
      pose proof (flight_measure_le (flight_after x r spent)) as Hf1.
      unfold after_fire. destruct spent eqn:Es; cbn [negb andb] in Hm.
      + (* FDone: the record of the fire is spent, and the settle goes on *)
        cbn [z_ops z_end].
        specialize (IH (S fired) (run st ops) (flight_after x r true) Hinv1 ltac:(lia) ltac:(lia)).
        cbn zeta in IH. destruct IH as [I1 [I2 [I3 I4]]].
        rewrite fires_app, Hops, run_app. split; [lia | split; [lia | split]].
        * intros H. specialize (I3 H). lia.
        * intros H. destruct (I4 H) as [w1 [r1 [Hin1 Hp1]]]. exists w1, r1.
          split; [apply in_or_app; right; exact Hin1 | exact Hp1].
      + destruct (w_pers w) eqn:Ew; cbn [andb] in Hm.
        * (* FDivergent *)
          cbn [z_ops z_end divergent_end]. rewrite fires_app, Hops, Hland0, run_app.
          rewrite (land_measure _ _ Hinv1).
          split; [lia | split; [lia | split; [intros H; discriminate |]]].
          intros _. exists w, r. split; [apply in_or_app; left; exact Hin | exact Ew].
        * destruct (cap <=? S fired) eqn:Ec.
          -- (* FRoundCap *)
             apply Nat.leb_le in Ec. cbn [z_ops z_end divergent_end].
             rewrite fires_app, Hops, Hland0, run_app, (land_measure _ _ Hinv1).
             split; [lia | split; [lia | split; [intros _; lia | intros H; discriminate]]].
          -- (* FContinue *)
             apply Nat.leb_gt in Ec. cbn [z_ops z_end].
             specialize (IH (S fired) (run st ops) (flight_after x r false) Hinv1 ltac:(lia) ltac:(lia)).
             cbn zeta in IH. destruct IH as [I1 [I2 [I3 I4]]].
             rewrite fires_app, Hops, run_app. split; [lia | split; [lia | split]].
             ++ intros H. specialize (I3 H). lia.
             ++ intros H. destruct (I4 H) as [w1 [r1 [Hin1 Hp1]]]. exists w1, r1.
                split; [apply in_or_app; right; exact Hin1 | exact Hp1].
  Qed.

  (* ── 16a. One fire count for each transaction (I-21) ───────────────── *)

  (* after_fire stops at the cap only when the count reaches the cap. *)
  Lemma after_fire_round_cap :
    forall n spent stays, after_fire n spent stays = FRoundCap -> cap <= n.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; cbn [negb] in H; [discriminate |].
    destruct stays; cbn [negb] in H; [discriminate |].
    destruct (cap <=? n) eqn:E; [apply Nat.leb_le; exact E | discriminate].
  Qed.

  (* after_fire continues only below the cap. *)
  Lemma after_fire_continue :
    forall n spent stays, after_fire n spent stays = FContinue -> n < cap.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; cbn [negb] in H; [discriminate |].
    destruct stays; cbn [negb] in H; [discriminate |].
    destruct (cap <=? n) eqn:E; [discriminate | apply Nat.leb_gt; exact E].
  Qed.

  Lemma consume_fire_fires : forall k h, fires (consume_fire k h) = 1.
  Proof. intros k h. unfold consume_fire. destruct (w_peek k && negb (r_pers h)); reflexivity. Qed.

  (* The cap bound of a settle that starts after fired fires of its
     transaction. It needs no premise on the view: the cap check at the top
     of each round stops the settle at the cap. *)
  Lemma settle_loop_cap :
    forall fuel fired st x,
      fired <= cap -> 2 * (cap - fired) + flight_measure x < fuel ->
      let z := settle_loop fuel fired st x in
      fired + fires (z_ops z) <= cap /\
      (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap).
  Proof.
    assert (Hland0 : forall x, fires (land x) = 0) by (intros [r |]; reflexivity).
    induction fuel as [| fuel IH]; intros fired st x Hle Hfuel z; [exfalso; lia |].
    subst z. cbn [settle_loop].
    destruct (next_fire st x) as [[[w r] b] |].
    2: { cbn [z_ops z_end]. rewrite Hland0. split; [lia | intros H; discriminate]. }
    destruct (lands_first x b) eqn:Hl.
    { pose proof (lands_first_flight x b Hl) as Hx.
      destruct (IH fired (run st (land x)) None Hle ltac:(cbn [flight_measure]; lia)) as [I1 I2].
      cbn [z_ops z_end]. rewrite fires_app, Hland0. split; [lia | intros H; specialize (I2 H); lia]. }
    destruct (at_cap fired) eqn:Hcap.
    { unfold at_cap in Hcap. apply Nat.leb_le in Hcap. cbn [z_ops z_end]. rewrite Hland0.
      split; [lia | intros _; lia]. }
    unfold at_cap in Hcap. apply Nat.leb_gt in Hcap.
    assert (Hops : fires (lift_ops x r ++ [SFireProduce w r]) = 1).
    { unfold lift_ops. destruct x; [| destruct (r_pers r)]; reflexivity. }
    pose proof (flight_measure_le (flight_after x r (negb (r_pers r) && negb (w_peek w)))) as Hf1.
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)) eqn:Ea;
      cbn [z_ops z_end].
    - destruct (IH (S fired) (run st (lift_ops x r ++ [SFireProduce w r]))
                  (flight_after x r (negb (r_pers r) && negb (w_peek w))) ltac:(lia) ltac:(lia))
        as [I1 I2].
      rewrite fires_app, Hops. split; [lia | intros H; specialize (I2 H); lia].
    - destruct (IH (S fired) (run st (lift_ops x r ++ [SFireProduce w r]))
                  (flight_after x r (negb (r_pers r) && negb (w_peek w))) ltac:(lia) ltac:(lia))
        as [I1 I2].
      rewrite fires_app, Hops. split; [lia | intros H; specialize (I2 H); lia].
    - rewrite fires_app, Hops, Hland0. split; [lia | intros H; discriminate].
    - apply after_fire_round_cap in Ea. rewrite fires_app, Hops, Hland0.
      split; [lia | intros _; lia].
  Qed.

  (* The bound of a plan that starts after f fires of its transaction. *)
  Definition plan_bounded (f : nat) (p : planned) : Prop :=
    f + fires (p_ops p) <= cap /\
    (p_end p = PReissue ReissueRoundCap -> f + fires (p_ops p) = cap).

  Lemma offer_loop_cap :
    forall fuel fired st r refused,
      fired < cap -> cap <= fired + fuel -> plan_bounded fired (offer_loop fuel fired st r refused).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r refused Hlt Hfuel; [exfalso; lia |].
    unfold plan_bounded. cbn [offer_loop].
    destruct (offer_pick (s_waiters st) r refused) as [[w |] refused'];
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)) eqn:Ea;
      cbn [p_ops p_end].
    - cbn [fires filter is_fire length]. split; [lia | intros H; discriminate].
    - apply after_fire_continue in Ea.
      destruct (IH (S fired) (apply st (SFireProduce w r)) r refused' Ea ltac:(lia)) as [I1 I2].
      rewrite fires_cons. cbn [is_fire]. split; [lia | intros H; specialize (I2 H); lia].
    - cbn [fires filter is_fire length]. split; [lia | intros H; discriminate].
    - apply after_fire_round_cap in Ea. cbn [fires filter is_fire length].
      split; [lia | intros _; lia].
  Qed.

  Lemma receive_loop_cap :
    forall fuel fired st k,
      fired < cap -> cap <= fired + fuel -> plan_bounded fired (receive_loop fuel fired st k).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hlt Hfuel; [exfalso; lia |].
    unfold plan_bounded. cbn [receive_loop].
    destruct (first_match (s_records st) (w_lane k) (w_pat k)) as [h |];
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    destruct (admits (w_guard k) (r_val h));
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    pose proof (consume_fire_fires k h) as Hf.
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)) eqn:Ea;
      cbn [p_ops p_end].
    - rewrite Hf. split; [lia | intros H; discriminate].
    - apply after_fire_continue in Ea.
      destruct (IH (S fired) (run st (consume_fire k h)) k Ea ltac:(lia)) as [I1 I2].
      rewrite fires_app, Hf. split; [lia | intros H; specialize (I2 H); lia].
    - rewrite Hf. split; [lia | intros H; discriminate].
    - apply after_fire_round_cap in Ea. rewrite Hf. split; [lia | intros _; lia].
  Qed.

  (* The cause of the next round. *)
  Definition next_cause (n : next_round) : reissue :=
    match n with NextOperation c _ => c | NextSettle c => c end.

  (* The bound of a transaction (I-21): at most cap fires, and exactly cap
     fires when the transaction re-issues at the cap. *)
  Definition txn_bounded (t : outcome) : Prop :=
    fires (o_ops t) <= cap /\
    (forall n, o_next t = Some n -> next_cause n = ReissueRoundCap -> fires (o_ops t) = cap).

  (* Headline 11. The settle terminates, and every transaction makes at most
     cap fires (I-21, TxnFiresBounded). A settle that starts after fired fires
     of its transaction makes at most cap - fired fires. Every fire lowers μ,
     except a final divergent fire, and a landing keeps μ. The measure μ counts
     the stored records and waiters that are not persistent and the record in
     flight. So the fires are at most μ, plus one for a divergent pair. A
     settle that stops at the cap brings its transaction to exactly cap fires.
     A divergent pair has a persistent waiter, and a settle that ends Quiescent
     leaves INV. The premise of these settle bounds is wf: distinct orders on
     each side. A produce, a consume, a settle job and a re-entry each make at
     most cap fires with their settles, with no premise. A transaction that
     re-issues at the cap made exactly cap fires. A settle job that completes
     in its round leaves INV. *)
  Theorem pq_settle_terminates :
    (forall fired st,
       ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) -> fired <= cap ->
       let z := settle_at fired st in
       fired + fires (z_ops z) <= cap /\
       fires (z_ops z) <= measure st + divergent_end (z_end z) /\
       (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap) /\
       (z_end z = SettleReissue ReissueDivergent ->
          exists w r, In (SFireProduce w r) (z_ops z) /\ w_pers w = true) /\
       (z_end z = Quiescent -> no_pending_match (run st (z_ops z)))) /\
    (forall st rank v lvl pers, txn_bounded (produce st rank v lvl pers)) /\
    (forall st rank pat g lane pers peek, txn_bounded (consume st rank pat g lane pers peek)) /\
    (forall st, txn_bounded (settle_job st)) /\
    (forall st x, txn_bounded (reenter_produce st x)) /\
    (forall st k, txn_bounded (reenter_consume st k)) /\
    (forall st, o_next (settle_job st) = None -> no_pending_match (after st (settle_job st))).
  Proof.
    assert (Hws : forall st pers o plan,
               (forall f s, f < cap -> plan_bounded f (plan f s)) ->
               txn_bounded (with_settles st pers o plan)).
    { intros st pers o plan Hplan. unfold with_settles, txn_bounded, settle_of.
      pose proof (settle_loop_cap settle_fuel 0 st None ltac:(lia)
                    ltac:(unfold settle_fuel; cbn [flight_measure]; lia)) as B.
      cbn zeta in B. destruct B as [B1 B2].
      destruct (z_end (settle_loop settle_fuel 0 st None)) as [| c] eqn:Eb.
      2: { cbn [o_ops o_next]. split; [lia |].
           intros n Hn Hc. injection Hn as <-. cbn [next_cause] in Hc. subst c.
           try rewrite Eb in B2. specialize (B2 eq_refl). lia. }
      set (B := z_ops (settle_loop settle_fuel 0 st None)) in *.
      destruct (plan_at_cap (fires B)) eqn:Ec.
      { apply plan_at_cap_true in Ec. cbn [o_ops o_next]. split; [lia | intros n Hn _; lia]. }
      unfold plan_at_cap in Ec. apply Nat.leb_gt in Ec.
      destruct (Hplan (fires B) (run st B) Ec) as [P1 P2].
      set (Pl := plan (fires B) (run st B)) in *.
      destruct (p_end Pl) as [| c] eqn:Ep; cbn [o_ops o_next].
      - lazymatch goal with
        | |- context [settle_at ?g ?s] =>
            pose proof (settle_loop_cap settle_fuel g s None ltac:(lia)
                          ltac:(unfold settle_fuel; cbn [flight_measure]; lia)) as A;
            cbn zeta in A; destruct A as [A1 A2];
            unfold settle_at; rewrite !fires_app; split; [lia |];
            intros n Hn Hc;
            destruct (z_end (settle_loop settle_fuel g s None)) as [| c'] eqn:Ea;
            cbn [settle_next] in Hn; [discriminate |];
            injection Hn as <-; cbn [next_cause] in Hc; subst c';
            try rewrite Ea in A2; specialize (A2 eq_refl); lia
        end.
      - rewrite fires_app. split; [lia |].
        intros n Hn Hc. injection Hn as <-. cbn [next_cause] in Hc. subst c.
        try rewrite Ep in P2. specialize (P2 eq_refl). lia. }
    split; [| split; [| split; [| split; [| split; [| split]]]]].
    - intros fired st Hr Hw Hle z. unfold z, settle_at.
      assert (Hinv : settle_inv st None) by (split; [exact Hr | split; [exact Hw | exact I]]).
      pose proof (settle_loop_bounds settle_fuel fired st None Hinv Hle
                    ltac:(unfold settle_fuel; cbn [flight_measure]; lia)) as B.
      cbn zeta in B. destruct B as [B1 [B2 [B3 B4]]]. cbn [flight_measure] in B2.
      split; [lia | split; [lia | split; [exact B3 | split; [exact B4 |]]]].
      intros H. apply settle_quiescent_inv. exact H.
    - intros st rank v lvl pers. unfold produce. apply Hws. intros f s Hf. unfold fresh_offer.
      destruct (record_head (s_records s) _) as [h |];
        [unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
         split; [lia | intros H; discriminate] |].
      exact (offer_loop_cap (S cap) f s _ _ Hf ltac:(lia)).
    - intros st rank pat g lane pers peek. unfold consume. apply Hws. intros f s Hf.
      unfold fresh_receive.
      destruct (waiter_head (s_waiters s) _) as [w |];
        [unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
         split; [lia | intros H; discriminate] |].
      exact (receive_loop_cap (S cap) f s _ Hf ltac:(lia)).
    - intros st. unfold settle_job, txn_bounded, settle_of.
      pose proof (settle_loop_cap settle_fuel 0 (bumped st) None ltac:(lia)
                    ltac:(unfold settle_fuel; cbn [flight_measure]; lia)) as B.
      cbn zeta in B. destruct B as [B1 B2]. cbn [o_ops o_next]. split; [lia |].
      intros n Hn Hc.
      destruct (z_end (settle_loop settle_fuel 0 (bumped st) None)) as [| c] eqn:Ea;
        cbn [settle_next] in Hn; [discriminate |].
      injection Hn as <-. cbn [next_cause] in Hc. subst c.
      try rewrite Ea in B2. specialize (B2 eq_refl). lia.
    - intros st x. unfold reenter_produce. apply Hws. intros f s Hf. unfold reenter_offer.
      destruct (record_head (s_records s) (r_lvl x)) as [h |]; [destruct (r_ord x <? r_ord h) |];
        first [exact (offer_loop_cap (S cap) f s x _ Hf ltac:(lia))
              | unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
                split; [lia | intros H; discriminate]].
    - intros st k. unfold reenter_consume. apply Hws. intros f s Hf. unfold reenter_receive.
      destruct (waiter_head (s_waiters s) (w_lane k)) as [w |]; [destruct (w_ord k <? w_ord w) |];
        first [exact (receive_loop_cap (S cap) f s k Hf ltac:(lia))
              | unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
                split; [lia | intros H; discriminate]].
    - intros st H. unfold after, settle_job, settle_of in *. cbn [o_ops o_next] in *.
      apply settle_next_none in H. apply settle_quiescent_inv. exact H.
  Qed.

  (* ── 17. The plans ignore the list order ────────────────────────────── *)

  (* Every order of the view is below n. *)
  Definition ords_below (n : nat) (st : sstate) : Prop :=
    (forall h, In h (s_records st) -> r_ord h < n) /\ (forall w, In w (s_waiters st) -> w_ord w < n).

  Lemma drop_record_below :
    forall n l h, (forall x, In x l -> r_ord x < n) -> forall x, In x (drop_record h l) -> r_ord x < n.
  Proof. intros n l h Hl x Hx. apply Hl. unfold drop_record in Hx. eapply drop_key_incl. exact Hx. Qed.

  Lemma drop_waiter_below :
    forall n l w, (forall x, In x l -> w_ord x < n) -> forall x, In x (drop_waiter w l) -> w_ord x < n.
  Proof. intros n l w Hl x Hx. apply Hl. unfold drop_waiter in Hx. eapply drop_key_incl. exact Hx. Qed.

  (* A settle keeps every order below n: it removes entries, and it stores
     again only records of the view. *)
  Lemma settle_ords_below :
    forall fuel fired st x n,
      ords_below n st -> (match x with Some r => r_ord r < n | None => True end) ->
      ords_below n (run st (z_ops (settle_loop fuel fired st x))).
  Proof.
    assert (Hland : forall st x n, ords_below n st ->
                      (match x with Some r => r_ord r < n | None => True end) ->
                      ords_below n (run st (land x))).
    { intros st [r |] n [Hr Hw] Hx; cbn [land run fold_left apply s_records s_waiters];
        [| split; assumption].
      split; [| exact Hw]. intros h [Hh | Hh]; [subst; exact Hx | exact (Hr h Hh)]. }
    induction fuel as [| fuel IH]; intros fired st x n Hst Hx; cbn [settle_loop].
    - apply Hland; assumption.
    - destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf; [| apply Hland; assumption].
      destruct (lands_first x b).
      + cbn [z_ops]. rewrite run_app. apply IH; [apply Hland; assumption | exact I].
      + destruct (at_cap fired); [apply Hland; assumption |].
        destruct (next_fire_some st x w r b Hnf) as [_ [_ [Hf _]]].
        destruct (fireable_spec _ _ _ Hf) as [Hm _].
        destruct (first_match_spec _ _ _ _ Hm) as [Hrin _].
        assert (Hrn : r_ord r < n).
        { destruct x as [r0 |]; cbn [view] in Hrin;
            [destruct Hrin as [Hrin | Hrin]; [subst; exact Hx | exact (proj1 Hst r Hrin)]
            | exact (proj1 Hst r Hrin)]. }
        assert (Hstep : ords_below n (run st (lift_ops x r ++ [SFireProduce w r]))).
        { destruct Hst as [Hrec Hwt]. split.
          - rewrite settle_step_records. destruct x; [exact Hrec |].
            destruct (r_pers r); [exact Hrec | apply drop_record_below; exact Hrec].
          - rewrite settle_step_waiters. destruct (w_pers w); [exact Hwt |].
            apply drop_waiter_below; exact Hwt. }
        assert (Hx1 : forall spent, match flight_after x r spent with
                                     | Some r1 => r_ord r1 < n | None => True end).
        { intros spent. destruct (flight_after x r spent) as [r1 |] eqn:Ef; [| exact I].
          destruct (flight_after_some x r spent r1 Ef) as [Hr1 _]. subst r1. exact Hrn. }
        destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
          cbn [z_ops]; rewrite run_app;
          solve [apply IH; [exact Hstep | apply Hx1] | apply Hland; [exact Hstep | apply Hx1]].
  Qed.

  (* Two views with the same entries in other list orders. *)
  Definition s_equiv (st1 st2 : sstate) : Prop :=
    Permutation (s_records st1) (s_records st2) /\ Permutation (s_waiters st1) (s_waiters st2) /\
    s_counter st1 = s_counter st2.

  (* Distinct orders on each side. *)
  Definition dist (st : sstate) : Prop :=
    ords_distinct r_ord (s_records st) /\ ords_distinct w_ord (s_waiters st).

  Lemma drop_key_perm :
    forall (E : Type) (ord rank : E -> nat) o k l1 l2,
      Permutation l1 l2 -> ords_distinct ord l1 ->
      Permutation (drop_key ord rank o k l1) (drop_key ord rank o k l2).
  Proof.
    intros E ord rank o k l1 l2 Hp Hnd.
    rewrite (drop_key_filter E ord rank o k l1 Hnd).
    rewrite (drop_key_filter E ord rank o k l2 (ords_distinct_perm E ord l1 l2 Hp Hnd)).
    apply filter_perm. exact Hp.
  Qed.

  Lemma apply_equiv :
    forall st1 st2 op, s_equiv st1 st2 -> dist st1 -> s_equiv (apply st1 op) (apply st2 op).
  Proof.
    intros st1 st2 op [Hr [Hw Hc]] [Dr Dw].
    destruct op as [r | w | w r | k h | h | r]; cbn [apply].
    - split; [apply perm_skip; exact Hr | split; assumption].
    - split; [exact Hr | split; [apply perm_skip; exact Hw | exact Hc]].
    - destruct (w_pers w); [split; [exact Hr | split; assumption] |].
      split; [exact Hr | split; [apply drop_key_perm; assumption | exact Hc]].
    - destruct (r_pers h); [split; [exact Hr | split; assumption] |].
      split; [apply drop_key_perm; assumption | split; assumption].
    - split; [apply drop_key_perm; assumption | split; assumption].
    - split; [apply perm_skip; exact Hr | split; assumption].
  Qed.

  Lemma view_perm : forall st1 st2 x, s_equiv st1 st2 -> Permutation (view st1 x) (view st2 x).
  Proof. intros st1 st2 [r |] [Hr _]; cbn [view]; [apply perm_skip |]; exact Hr. Qed.

  Lemma next_fire_equiv :
    forall st1 st2 x, s_equiv st1 st2 -> settle_inv st1 x -> next_fire st1 x = next_fire st2 x.
  Proof.
    intros st1 st2 x He Hinv.
    pose proof (view_distinct st1 x Hinv) as Hvd.
    pose proof (view_perm st1 st2 x He) as Hv.
    destruct He as [_ [Hw _]]. destruct Hinv as [_ [Dw _]].
    assert (Hf : forall u, fireable (view st1 x) u = fireable (view st2 x) u)
      by (intros u; exact (fireable_perm _ _ u Hv Hvd)).
    assert (Hh : forall u, is_waiter_head (s_waiters st1) u = is_waiter_head (s_waiters st2) u)
      by (intros u; exact (is_waiter_head_perm _ _ u Hw Dw)).
    assert (Hc : Permutation (fire_candidates st1 x) (fire_candidates st2 x)).
    { unfold fire_candidates.
      rewrite (filter_ext (fun w => is_waiter_head (s_waiters st2) w && has (fireable (view st2 x) w))
                          (fun w => is_waiter_head (s_waiters st1) w && has (fireable (view st1 x) w))).
      - apply filter_perm. exact Hw.
      - intros u. rewrite Hh, Hf. reflexivity. }
    unfold next_fire, oldest.
    rewrite (least_perm _ w_ord w_rank _ _ Hc).
    2: { apply keys_of_ords. unfold fire_candidates. apply ords_distinct_filter. exact Dw. }
    destruct (least w_ord w_rank (fire_candidates st2 x)) as [w |]; [| reflexivity].
    rewrite Hf. reflexivity.
  Qed.

  Lemma land_equiv :
    forall st1 st2 x, s_equiv st1 st2 -> settle_inv st1 x ->
      s_equiv (run st1 (land x)) (run st2 (land x)) /\ settle_inv (run st1 (land x)) None.
  Proof.
    intros st1 st2 x He Hinv. split; [| exact (land_inv st1 x Hinv)].
    destruct x as [r |]; cbn [land run fold_left apply]; [| exact He].
    destruct He as [Hr [Hw Hc]]. split; [apply perm_skip; exact Hr | split; assumption].
  Qed.

  Lemma settle_inv_dist : forall st, settle_inv st None -> dist st.
  Proof. intros st [Dr [Dw _]]. split; assumption. Qed.

  Lemma settle_inv_of_dist : forall st, dist st -> settle_inv st None.
  Proof. intros st [Dr Dw]. split; [exact Dr | split; [exact Dw | exact I]]. Qed.

  (* A settle on two views with the same entries stages the same
     operations, and it reaches views with the same entries. *)
  Lemma settle_loop_equiv :
    forall fuel fired st1 st2 x,
      s_equiv st1 st2 -> settle_inv st1 x ->
      settle_loop fuel fired st1 x = settle_loop fuel fired st2 x /\
      s_equiv (run st1 (z_ops (settle_loop fuel fired st1 x)))
              (run st2 (z_ops (settle_loop fuel fired st1 x))) /\
      dist (run st1 (z_ops (settle_loop fuel fired st1 x))).
  Proof.
    assert (Hlanded : forall st1 st2 x, s_equiv st1 st2 -> settle_inv st1 x ->
                        s_equiv (run st1 (land x)) (run st2 (land x)) /\ dist (run st1 (land x))).
    { intros st1 st2 x He Hinv. destruct (land_equiv st1 st2 x He Hinv) as [E D].
      split; [exact E | exact (settle_inv_dist _ D)]. }
    induction fuel as [| fuel IH]; intros fired st1 st2 x He Hinv; cbn [settle_loop].
    - split; [reflexivity |]. exact (Hlanded st1 st2 x He Hinv).
    - rewrite <- (next_fire_equiv st1 st2 x He Hinv).
      destruct (next_fire st1 x) as [[[w r] b] |] eqn:Hnf;
        [| split; [reflexivity | exact (Hlanded st1 st2 x He Hinv)]].
      destruct (lands_first x b) eqn:Hl.
      + destruct (land_equiv st1 st2 x He Hinv) as [E D].
        destruct (IH fired _ _ None E D) as [Eq [Hq Hd]].
        cbn [z_ops]. rewrite <- Eq. rewrite !run_app. split; [reflexivity | split; assumption].
      + destruct (at_cap fired); [split; [reflexivity | exact (Hlanded st1 st2 x He Hinv)] |].
        assert (Hd : dist st1) by (destruct Hinv as [Dr [Dw _]]; split; assumption).
        set (ops := lift_ops x r ++ [SFireProduce w r]).
        assert (Hops : s_equiv (run st1 ops) (run st2 ops)).
        { unfold ops, lift_ops. destruct x as [r0 |]; [| destruct (r_pers r)];
            cbn [app run fold_left].
          - apply apply_equiv; [exact He | exact Hd].
          - apply apply_equiv; [exact He | exact Hd].
          - apply apply_equiv; [apply apply_equiv; [exact He | exact Hd] |].
            unfold dist. cbn [apply s_records s_waiters]. unfold drop_record.
            split; [apply drop_key_distinct; exact (proj1 Hd) | exact (proj2 Hd)]. }
        assert (Hinv1 : forall spent, settle_inv (run st1 ops) (flight_after x r spent))
          by (intros spent; apply (settle_step_inv st1 x w r b); assumption).
        set (spent := negb (r_pers r) && negb (w_peek w)).
        destruct (after_fire (S fired) spent (w_pers w)).
        * destruct (IH (S fired) (run st1 ops) (run st2 ops) (flight_after x r spent) Hops (Hinv1 spent))
            as [Eq [Hequiv Hdist]].
          rewrite <- Eq. cbn [z_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
        * destruct (IH (S fired) (run st1 ops) (run st2 ops) (flight_after x r spent) Hops (Hinv1 spent))
            as [Eq [Hequiv Hdist]].
          rewrite <- Eq. cbn [z_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
        * cbn [z_ops]. rewrite !run_app. split; [reflexivity |].
          apply Hlanded; [exact Hops | apply Hinv1].
        * cbn [z_ops]. rewrite !run_app. split; [reflexivity |].
          apply Hlanded; [exact Hops | apply Hinv1].
  Qed.

  Lemma dist_cons_record :
    forall r l, ords_distinct r_ord l -> ~ In (r_ord r) (map r_ord l) -> ords_distinct r_ord (r :: l).
  Proof. intros r l Hnd Hnot. unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption. Qed.

  Lemma dist_cons_waiter :
    forall w l, ords_distinct w_ord l -> ~ In (w_ord w) (map w_ord l) -> ords_distinct w_ord (w :: l).
  Proof. intros w l Hnd Hnot. unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption. Qed.

  Lemma store_equiv :
    forall st1 st2 r, s_equiv st1 st2 -> dist st1 -> ~ In (r_ord r) (map r_ord (s_records st1)) ->
      s_equiv (run st1 [SStore r]) (run st2 [SStore r]) /\ dist (run st1 [SStore r]).
  Proof.
    intros st1 st2 r [Hr [Hw Hc]] [Dr Dw] Hfresh. cbn [run fold_left apply].
    split; [split; [apply perm_skip; exact Hr | split; assumption] |].
    split; [apply dist_cons_record; assumption | exact Dw].
  Qed.

  Lemma park_equiv :
    forall st1 st2 k, s_equiv st1 st2 -> dist st1 -> ~ In (w_ord k) (map w_ord (s_waiters st1)) ->
      s_equiv (run st1 [SPark k]) (run st2 [SPark k]) /\ dist (run st1 [SPark k]).
  Proof.
    intros st1 st2 k [Hr [Hw Hc]] [Dr Dw] Hfresh. cbn [run fold_left apply].
    split; [split; [exact Hr | split; [apply perm_skip; exact Hw | exact Hc]] |].
    split; [exact Dr | apply dist_cons_waiter; assumption].
  Qed.

  Lemma fire_produce_equiv :
    forall st1 st2 w r, s_equiv st1 st2 -> dist st1 ->
      s_equiv (apply st1 (SFireProduce w r)) (apply st2 (SFireProduce w r)) /\
      dist (apply st1 (SFireProduce w r)).
  Proof.
    intros st1 st2 w r He Hd. split; [apply apply_equiv; assumption |].
    cbn [apply]. destruct (w_pers w); [exact Hd |]. unfold dist. cbn [s_records s_waiters].
    unfold drop_waiter. split; [exact (proj1 Hd) | apply drop_key_distinct; exact (proj2 Hd)].
  Qed.

  Lemma offer_pick_equiv :
    forall ws1 ws2 r refused, Permutation ws1 ws2 -> ords_distinct w_ord ws1 ->
      offer_pick ws1 r refused = offer_pick ws2 r refused.
  Proof.
    intros ws1 ws2 r [ref1 ref2] Hp Hnd. unfold offer_pick, cand1, cand2.
    rewrite (waiter_head_perm ws1 ws2 None Hp Hnd).
    destruct (r_lvl r) as [p |]; [rewrite (waiter_head_perm ws1 ws2 (Some p) Hp Hnd) |];
      reflexivity.
  Qed.

  Lemma offer_loop_equiv :
    forall fuel fired st1 st2 r refused,
      s_equiv st1 st2 -> dist st1 -> ~ In (r_ord r) (map r_ord (s_records st1)) ->
      offer_loop fuel fired st1 r refused = offer_loop fuel fired st2 r refused /\
      s_equiv (run st1 (p_ops (offer_loop fuel fired st1 r refused)))
              (run st2 (p_ops (offer_loop fuel fired st1 r refused))) /\
      dist (run st1 (p_ops (offer_loop fuel fired st1 r refused))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st1 st2 r refused He Hd Hfresh; cbn [offer_loop].
    - split; [reflexivity | split; [exact He | exact Hd]].
    - rewrite <- (offer_pick_equiv (s_waiters st1) (s_waiters st2) r refused (proj1 (proj2 He))
                    (proj2 Hd)).
      pose proof (store_equiv st1 st2 r He Hd Hfresh) as Hstore.
      destruct (offer_pick (s_waiters st1) r refused) as [[w |] refused'];
        [| split; [reflexivity | exact Hstore]].
      destruct (fire_produce_equiv st1 st2 w r He Hd) as [Hfe Hfd].
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        try (split; [reflexivity | cbn [p_ops run fold_left]; split; assumption]).
      assert (Hfresh1 : ~ In (r_ord r) (map r_ord (s_records (apply st1 (SFireProduce w r)))))
        by (rewrite fire_produce_records; exact Hfresh).
      destruct (IH (S fired) _ _ r refused' Hfe Hfd Hfresh1) as [Eq [Hq Hdd]].
      rewrite <- Eq. cbn [p_ops]. rewrite !run_cons. split; [reflexivity | split; assumption].
  Qed.

  Lemma consume_fire_equiv :
    forall st1 st2 k h,
      s_equiv st1 st2 -> dist st1 -> In h (s_records st1) ->
      s_equiv (run st1 (consume_fire k h)) (run st2 (consume_fire k h)) /\
      dist (run st1 (consume_fire k h)).
  Proof.
    intros st1 st2 k h He Hd Hin.
    assert (Hdrop : dist (apply st1 (SFireConsume k h))).
    { cbn [apply]. destruct (r_pers h); [exact Hd |]. unfold dist. cbn [s_records s_waiters].
      unfold drop_record. split; [apply drop_key_distinct; exact (proj1 Hd) | exact (proj2 Hd)]. }
    unfold consume_fire. destruct (w_peek k && negb (r_pers h)) eqn:Ek.
    - apply andb_true_iff in Ek as [_ Ep]. apply negb_true_iff in Ep.
      cbn [run fold_left]. split.
      + apply apply_equiv; [apply apply_equiv; assumption | exact Hdrop].
      + cbn [apply]. rewrite Ep. unfold dist. cbn [s_records s_waiters].
        split; [| exact (proj2 Hd)]. apply dist_cons_record.
        * unfold drop_record. apply drop_key_distinct. exact (proj1 Hd).
        * unfold drop_record. apply drop_key_absent; [exact (proj1 Hd) | exact Hin].
    - cbn [run fold_left]. split; [apply apply_equiv; assumption | exact Hdrop].
  Qed.

  Lemma receive_loop_equiv :
    forall fuel fired st1 st2 k,
      s_equiv st1 st2 -> dist st1 -> ~ In (w_ord k) (map w_ord (s_waiters st1)) ->
      receive_loop fuel fired st1 k = receive_loop fuel fired st2 k /\
      s_equiv (run st1 (p_ops (receive_loop fuel fired st1 k)))
              (run st2 (p_ops (receive_loop fuel fired st1 k))) /\
      dist (run st1 (p_ops (receive_loop fuel fired st1 k))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st1 st2 k He Hd Hfresh; cbn [receive_loop].
    - split; [reflexivity | split; [exact He | exact Hd]].
    - rewrite <- (first_match_perm (s_records st1) (s_records st2) (w_lane k) (w_pat k)
                    (proj1 He) (proj1 Hd)).
      pose proof (park_equiv st1 st2 k He Hd Hfresh) as Hpark.
      destruct (first_match (s_records st1) (w_lane k) (w_pat k)) as [h |] eqn:Hm;
        [| split; [reflexivity | exact Hpark]].
      destruct (first_match_spec _ _ _ _ Hm) as [Hin _].
      destruct (admits (w_guard k) (r_val h)); [| split; [reflexivity | exact Hpark]].
      destruct (consume_fire_equiv st1 st2 k h He Hd Hin) as [Hfe Hfd].
      destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
        try (split; [reflexivity | split; assumption]).
      assert (Hfresh1 : ~ In (w_ord k) (map w_ord (s_waiters (run st1 (consume_fire k h)))))
        by (rewrite consume_fire_waiters; exact Hfresh).
      destruct (IH (S fired) _ _ k Hfe Hfd Hfresh1) as [Eq [Hq Hdd]].
      rewrite <- Eq. cbn [p_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
  Qed.

  (* The plans of both settles and of the operation ignore the list order.
     Both views reach the plan with one fire count, so the plan and the
     settle after it start at one count on both views. *)
  Lemma with_settles_equiv :
    forall st1 st2 pers o plan (fresh : sstate -> Prop),
      s_equiv st1 st2 -> dist st1 ->
      (forall f s1 s2, s_equiv s1 s2 -> dist s1 -> fresh s1 ->
         plan f s1 = plan f s2 /\
         s_equiv (run s1 (p_ops (plan f s1))) (run s2 (p_ops (plan f s1))) /\
         dist (run s1 (p_ops (plan f s1)))) ->
      fresh (run st1 (z_ops (settle_of st1))) ->
      with_settles st1 pers o plan = with_settles st2 pers o plan /\
      s_equiv (run st1 (o_ops (with_settles st1 pers o plan)))
              (run st2 (o_ops (with_settles st1 pers o plan))).
  Proof.
    intros st1 st2 pers o plan fresh He Hd Hplan Hfresh.
    destruct (settle_loop_equiv settle_fuel 0 st1 st2 None He (settle_inv_of_dist st1 Hd))
      as [Eb [Hbe Hbd]].
    unfold with_settles, settle_of in *. rewrite <- Eb.
    set (B := z_ops (settle_loop settle_fuel 0 st1 None)) in *.
    destruct (z_end (settle_loop settle_fuel 0 st1 None)); [| split; [reflexivity | exact Hbe]].
    destruct (plan_at_cap (fires B)); [split; [reflexivity | exact Hbe] |].
    lazymatch goal with
    | |- context [plan ?f (run st1 B)] =>
        destruct (Hplan f _ _ Hbe Hbd Hfresh) as [Ep [Hpe Hpd]]; rewrite <- Ep;
        set (Pops := p_ops (plan f (run st1 B))) in *;
        destruct (p_end (plan f (run st1 B)))
    end.
    - unfold settle_at.
      lazymatch goal with
      | |- context [settle_loop settle_fuel ?g _ None] =>
          destruct (settle_loop_equiv settle_fuel g _ _ None Hpe (settle_inv_of_dist _ Hpd))
            as [Ea [Hae _]]
      end.
      cbn [o_ops]. rewrite !run_app. rewrite <- Ea. split; [reflexivity | exact Hae].
    - cbn [o_ops]. rewrite !run_app. split; [reflexivity | exact Hpe].
  Qed.

  Lemma below_fresh_record : forall n st, ords_below n st -> ~ In n (map r_ord (s_records st)).
  Proof.
    intros n st [Hr _] Hin. apply in_map_iff in Hin as [h [Hh Hin]].
    specialize (Hr h Hin). lia.
  Qed.

  Lemma below_fresh_waiter : forall n st, ords_below n st -> ~ In n (map w_ord (s_waiters st)).
  Proof.
    intros n st [_ Hw] Hin. apply in_map_iff in Hin as [w [Hw' Hin]].
    specialize (Hw w Hin). lia.
  Qed.

  Lemma fresh_offer_equiv :
    forall x f s1 s2, s_equiv s1 s2 -> dist s1 -> ~ In (r_ord x) (map r_ord (s_records s1)) ->
      fresh_offer x f s1 = fresh_offer x f s2 /\
      s_equiv (run s1 (p_ops (fresh_offer x f s1))) (run s2 (p_ops (fresh_offer x f s1))) /\
      dist (run s1 (p_ops (fresh_offer x f s1))).
  Proof.
    intros x f s1 s2 He Hd Hfresh. unfold fresh_offer.
    rewrite <- (record_head_perm (s_records s1) (s_records s2) (r_lvl x) (proj1 He) (proj1 Hd)).
    destruct (record_head (s_records s1) (r_lvl x)) as [h |].
    - split; [reflexivity |]. cbn [p_ops]. exact (store_equiv s1 s2 x He Hd Hfresh).
    - exact (offer_loop_equiv (S cap) f s1 s2 x (false, false) He Hd Hfresh).
  Qed.

  Lemma fresh_receive_equiv :
    forall k f s1 s2, s_equiv s1 s2 -> dist s1 -> ~ In (w_ord k) (map w_ord (s_waiters s1)) ->
      fresh_receive k f s1 = fresh_receive k f s2 /\
      s_equiv (run s1 (p_ops (fresh_receive k f s1))) (run s2 (p_ops (fresh_receive k f s1))) /\
      dist (run s1 (p_ops (fresh_receive k f s1))).
  Proof.
    intros k f s1 s2 He Hd Hfresh. unfold fresh_receive.
    rewrite <- (waiter_head_perm (s_waiters s1) (s_waiters s2) (w_lane k) (proj1 (proj2 He))
                  (proj2 Hd)).
    destruct (waiter_head (s_waiters s1) (w_lane k)) as [w |].
    - split; [reflexivity |]. cbn [p_ops]. exact (park_equiv s1 s2 k He Hd Hfresh).
    - exact (receive_loop_equiv (S cap) f s1 s2 k He Hd Hfresh).
  Qed.

  (* Headline 12. The plans ignore the list order. The hot store inserts at the
     front, and a restart can load another order. Take two views with the same
     entries in other list orders. They stage the same operations in a produce,
     a consume and a settle job. They take the same next round and reach views
     with the same entries. The premises are wf: distinct orders on each side
     and orders below the counter. *)
  Theorem pq_plan_permutation_invariant :
    forall st1 st2,
      s_equiv st1 st2 -> dist st1 -> ords_below (bump_ord (s_counter st1)) st1 ->
      (forall rank v lvl pers,
         produce st1 rank v lvl pers = produce st2 rank v lvl pers /\
         s_equiv (after st1 (produce st1 rank v lvl pers)) (after st2 (produce st2 rank v lvl pers))) /\
      (forall rank pat g lane pers peek,
         consume st1 rank pat g lane pers peek = consume st2 rank pat g lane pers peek /\
         s_equiv (after st1 (consume st1 rank pat g lane pers peek))
                 (after st2 (consume st2 rank pat g lane pers peek))) /\
      (settle_job st1 = settle_job st2 /\
       s_equiv (after st1 (settle_job st1)) (after st2 (settle_job st2))).
  Proof.
    intros st1 st2 He Hd Hb.
    assert (Hc : s_counter st1 = s_counter st2) by (destruct He as [_ [_ Hc]]; exact Hc).
    assert (Hbe : s_equiv (bumped st1) (bumped st2)).
    { destruct He as [Hr [Hw _]]. unfold bumped.
      split; [exact Hr | split; [exact Hw | cbn [s_counter]; rewrite Hc; reflexivity]]. }
    assert (Hbd : dist (bumped st1)) by exact Hd.
    assert (Hbb : ords_below (bump_ord (s_counter st1)) (bumped st1)) by exact Hb.
    split; [| split].
    - intros rank v lvl pers. unfold produce, after. cbn zeta. rewrite <- Hc.
      destruct (with_settles_equiv (bumped st1) (bumped st2) pers (bump_ord (s_counter st1))
                  (fresh_offer (mkRecord (bump_ord (s_counter st1)) rank lvl v pers))
                  (fun s => ~ In (bump_ord (s_counter st1)) (map r_ord (s_records s))) Hbe Hbd)
        as [Eq Heq].
      + intros f s1 s2 Hs Hsd Hsf.
        exact (fresh_offer_equiv (mkRecord (bump_ord (s_counter st1)) rank lvl v pers) f s1 s2
                 Hs Hsd Hsf).
      + apply below_fresh_record. apply settle_ords_below; [exact Hbb | exact I].
      + split; [exact Eq | rewrite <- Eq; exact Heq].
    - intros rank pat g lane pers peek. unfold consume, after. cbn zeta. rewrite <- Hc.
      destruct (with_settles_equiv (bumped st1) (bumped st2) pers (bump_ord (s_counter st1))
                  (fresh_receive (mkWaiter (bump_ord (s_counter st1)) rank lane pat g pers peek))
                  (fun s => ~ In (bump_ord (s_counter st1)) (map w_ord (s_waiters s))) Hbe Hbd)
        as [Eq Heq].
      + intros f s1 s2 Hs Hsd Hsf.
        exact (fresh_receive_equiv (mkWaiter (bump_ord (s_counter st1)) rank lane pat g pers peek)
                 f s1 s2 Hs Hsd Hsf).
      + apply below_fresh_waiter. apply settle_ords_below; [exact Hbb | exact I].
      + split; [exact Eq | rewrite <- Eq; exact Heq].
    - unfold settle_job, after, settle_of.
      destruct (settle_loop_equiv settle_fuel 0 (bumped st1) (bumped st2) None Hbe
                  (settle_inv_of_dist _ Hbd)) as [Eq [Heq _]].
      rewrite <- Eq. split; [reflexivity | exact Heq].
  Qed.

  (* ── 17a. The domain of the levels and the storage of a level ─────────── *)

  (* The check of check_values in priority_queue.rs. The value of a priority
     param is a GInt p with 0 <= p. It is below the bound priorities when the
     space sets one (I-10). Without the param, there is no bound (FACTORY D3). *)
  Definition priority_admitted (bound : option nat) (p : Z) : bool :=
    (0 <=? p)%Z && match bound with Some n => (p <? Z.of_nat n)%Z | None => true end.

  (* A send whose priority param is checked before the bump. A priority out of
     the domain fails with InvalidParam and changes nothing (None). A send
     without a priority goes to the unprioritized level. *)
  Definition produce_checked (bound : option nat) (st : sstate) (rank : nat) (v : V)
    (priority : option Z) (pers : bool) : option outcome :=
    match priority with
    | None => Some (produce st rank v None pers)
    | Some p =>
        if priority_admitted bound p then Some (produce st rank v (Some (Z.to_nat p)) pers)
        else None
    end.

  (* Headline 14. The domain of the priorities. This is the port of the valid
     and the invalid priorities of the legacy priority queue. The default level
     of the legacy queue, a hypothesis there, is the unprioritized level here.
     A priority is admitted exactly when it is a natural number below the bound
     of the space, when the space sets one. A send with an admitted priority p
     runs at the level p. A send with a priority out of the domain fails before
     the bump and changes nothing. A send without a priority runs at the
     unprioritized level. *)
  Theorem pq_priority_domain :
    (forall bound p, priority_admitted bound p = true <->
       (0 <= p)%Z /\ forall n, bound = Some n -> (p < Z.of_nat n)%Z) /\
    (forall bound st rank v p pers, priority_admitted bound p = false ->
       produce_checked bound st rank v (Some p) pers = None) /\
    (forall bound st rank v p pers, priority_admitted bound p = true ->
       exists q, Z.of_nat q = p /\
         produce_checked bound st rank v (Some p) pers = Some (produce st rank v (Some q) pers)) /\
    (forall bound st rank v pers,
       produce_checked bound st rank v None pers = Some (produce st rank v None pers)).
  Proof.
    split; [| split; [| split]].
    - intros bound p. unfold priority_admitted. rewrite andb_true_iff, Z.leb_le. split.
      + intros [H0 Hb]. split; [exact H0 |]. intros n Hn. subst bound.
        apply Z.ltb_lt. exact Hb.
      + intros [H0 Hb]. split; [exact H0 |]. destruct bound as [n |]; [| reflexivity].
        apply Z.ltb_lt. apply Hb. reflexivity.
    - intros bound st rank v p pers H. unfold produce_checked. rewrite H. reflexivity.
    - intros bound st rank v p pers H. exists (Z.to_nat p). split.
      + unfold priority_admitted in H. apply andb_true_iff in H as [H0 _]. apply Z.leb_le in H0.
        apply Z2Nat.id. exact H0.
      + unfold produce_checked. rewrite H. reflexivity.
    - reflexivity.
  Qed.

  (* The size of a view: its records and its waiters. A priority queue keeps
     no other storage (I-10): the level of a record lives in its envelope. *)
  Definition size (st : sstate) : nat := length (s_records st) + length (s_waiters st).

  Lemma drop_key_length_le :
    forall (E : Type) (ord rank : E -> nat) o k l, length (drop_key ord rank o k l) <= length l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; [cbn; lia |].
    cbn [drop_key]. destruct ((ord y =? o) && (rank y =? k)); cbn [length]; lia.
  Qed.

  Lemma drop_key_length_in :
    forall (E : Type) (ord rank : E -> nat) l x,
      In x l -> S (length (drop_key ord rank (ord x) (rank x) l)) = length l.
  Proof.
    intros E ord rank l x. induction l as [| y rest IH]; intros Hx; [destruct Hx |].
    cbn [drop_key]. destruct ((ord y =? ord x) && (rank y =? rank x)) eqn:Ek; [reflexivity |].
    destruct Hx as [Hx | Hx]; [subst y; rewrite !Nat.eqb_refl in Ek; discriminate |].
    cbn [length]. rewrite (IH Hx). reflexivity.
  Qed.

  Lemma fire_produce_size : forall st w r, size (apply st (SFireProduce w r)) <= size st.
  Proof.
    intros st w r. unfold size. cbn [apply]. destruct (w_pers w); [lia |].
    cbn [s_records s_waiters]. unfold drop_waiter.
    pose proof (drop_key_length_le waiter w_ord w_rank (w_ord w) (w_rank w) (s_waiters st)). lia.
  Qed.

  Lemma land_size : forall st x, size (run st (land x)) = size st + flight_measure x.
  Proof. intros st [r |]; unfold size; cbn; lia. Qed.

  (* A settle adds no entry: a lift removes a stored record, the record is
     in flight until it lands, and a fire removes entries. *)
  Lemma settle_size :
    forall fuel fired st x,
      size (run st (z_ops (settle_loop fuel fired st x))) <= size st + flight_measure x.
  Proof.
    induction fuel as [| fuel IH]; intros fired st x; cbn [settle_loop].
    - cbn [z_ops]. rewrite land_size. lia.
    - destruct (next_fire st x) as [[[w r] b] |] eqn:Hnf; [| cbn [z_ops]; rewrite land_size; lia].
      destruct (lands_first x b).
      + cbn [z_ops]. rewrite run_app. specialize (IH fired (run st (land x)) None).
        rewrite land_size in IH. cbn [flight_measure] in IH. lia.
      + destruct (at_cap fired); [cbn [z_ops]; rewrite land_size; lia |].
        destruct (next_fire_some st x w r b Hnf) as [_ [_ [Hf _]]].
        destruct (fireable_spec _ _ _ Hf) as [Hm _].
        destruct (first_match_spec _ _ _ _ Hm) as [Hrin _].
        assert (Hstep : forall spent,
                  size (run st (lift_ops x r ++ [SFireProduce w r]))
                  + flight_measure (flight_after x r spent) <= size st + flight_measure x).
        { intros spent. pose proof (flight_measure_le (flight_after x r spent)) as Hf1.
          unfold lift_ops. destruct x as [r0 |].
          - cbn [app run fold_left]. pose proof (fire_produce_size st w r). cbn [flight_measure]. lia.
          - cbn [view] in Hrin. destruct (r_pers r) eqn:Ep.
            + cbn [app run fold_left]. pose proof (fire_produce_size st w r).
              unfold flight_after, flying. rewrite Ep. cbn [negb andb flight_measure]. lia.
            + cbn [app run fold_left].
              pose proof (fire_produce_size (apply st (SLift r)) w r) as Hp.
              assert (Hl : S (size (apply st (SLift r))) = size st).
              { unfold size. cbn [apply s_records s_waiters]. unfold drop_record.
                rewrite <- (drop_key_length_in record r_ord r_rank (s_records st) r Hrin). lia. }
              cbn [flight_measure]. lia. }
        destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
          cbn [z_ops]; rewrite run_app;
          [ specialize (IH (S fired) (run st (lift_ops x r ++ [SFireProduce w r]))
                          (flight_after x r (negb (r_pers r) && negb (w_peek w))))
          | specialize (IH (S fired) (run st (lift_ops x r ++ [SFireProduce w r]))
                          (flight_after x r (negb (r_pers r) && negb (w_peek w))))
          | rewrite land_size | rewrite land_size ];
          pose proof (Hstep (negb (r_pers r) && negb (w_peek w))); lia.
  Qed.

  Lemma offer_loop_size :
    forall fuel fired st r refused,
      size (run st (p_ops (offer_loop fuel fired st r refused))) <= S (size st).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r refused; cbn [offer_loop]; [cbn; lia |].
    destruct (offer_pick (s_waiters st) r refused) as [[w |] refused'];
      [| unfold size; cbn; lia].
    pose proof (fire_produce_size st w r) as Hf.
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [p_ops run fold_left]; try lia.
    specialize (IH (S fired) (apply st (SFireProduce w r)) r refused'). unfold run in IH. lia.
  Qed.

  Lemma consume_fire_size :
    forall st k h, In h (s_records st) -> size (run st (consume_fire k h)) <= size st.
  Proof.
    intros st k h Hin. unfold consume_fire.
    destruct (w_peek k && negb (r_pers h)) eqn:Ek.
    - apply andb_true_iff in Ek as [_ Ep]. apply negb_true_iff in Ep.
      unfold size. cbn [run fold_left apply]. rewrite Ep. cbn [s_records s_waiters length].
      unfold drop_record. rewrite (drop_key_length_in record r_ord r_rank (s_records st) h Hin). lia.
    - unfold size. cbn [run fold_left apply]. destruct (r_pers h); [lia |].
      cbn [s_records s_waiters]. unfold drop_record.
      pose proof (drop_key_length_le record r_ord r_rank (r_ord h) (r_rank h) (s_records st)). lia.
  Qed.

  Lemma receive_loop_size :
    forall fuel fired st k, size (run st (p_ops (receive_loop fuel fired st k))) <= S (size st).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k; cbn [receive_loop]; [cbn; lia |].
    destruct (first_match (s_records st) (w_lane k) (w_pat k)) as [h |] eqn:Hm;
      [| unfold size; cbn; lia].
    destruct (first_match_spec _ _ _ _ Hm) as [Hin _].
    destruct (admits (w_guard k) (r_val h)); [| unfold size; cbn; lia].
    pose proof (consume_fire_size st k h Hin) as Hf.
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)); cbn [p_ops]; try lia.
    rewrite run_app. specialize (IH (S fired) (run st (consume_fire k h)) k). lia.
  Qed.

  Lemma with_settles_size :
    forall st pers o plan,
      (forall f s, size (run s (p_ops (plan f s))) <= S (size s)) ->
      size (run st (o_ops (with_settles st pers o plan))) <= S (size st).
  Proof.
    intros st pers o plan Hplan. unfold with_settles.
    assert (Hbefore : size (run st (z_ops (settle_of st))) <= size st)
      by (unfold settle_of; pose proof (settle_size settle_fuel 0 st None) as H;
          cbn [flight_measure] in H; lia).
    destruct (z_end (settle_of st)); cbn [o_ops]; [| lia].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_ops]; [lia |].
    match goal with |- context [p_end ?p] => destruct (p_end p) end; cbn [o_ops].
    - rewrite !run_app. unfold settle_at.
      lazymatch goal with
      | |- context [settle_loop settle_fuel ?g ?s None] =>
          pose proof (settle_size settle_fuel g s None) as Ha; cbn [flight_measure] in Ha
      end.
      lazymatch goal with
      | |- context [p_ops (plan ?f ?s)] => pose proof (Hplan f s) as Hp
      end.
      lia.
    - rewrite run_app.
      lazymatch goal with
      | |- context [p_ops (plan ?f ?s)] => pose proof (Hplan f s) as Hp
      end.
      lia.
  Qed.

  Lemma bumped_size : forall st, size (bumped st) = size st.
  Proof. reflexivity. Qed.

  (* Headline 15. A priority queue keeps no storage for a level (I-10, the
     port that removes the legacy ensure_capacity). A level exists exactly
     when a record holds it: the level has a head exactly when a record of the
     view is at that level. An operation adds at most one entry to the view,
     whatever its priority, and a settle job adds none. So a send with a
     large priority allocates nothing for the levels below it. *)
  Theorem pq_no_level_storage :
    (forall l lvl, record_head l lvl <> None <-> exists r, In r l /\ r_lvl r = lvl) /\
    (forall st rank v lvl pers, size (after st (produce st rank v lvl pers)) <= S (size st)) /\
    (forall st rank pat g lane pers peek,
       size (after st (consume st rank pat g lane pers peek)) <= S (size st)) /\
    (forall st, size (after st (settle_job st)) <= size st).
  Proof.
    split; [| split; [| split]].
    - intros l lvl. split.
      + intros H. destruct (record_head l lvl) as [h |] eqn:E; [| contradiction].
        destruct (record_head_spec l lvl h E) as [Hin [Hl _]]. exists h. split; assumption.
      + intros [r [Hr Hl]] H. exact (proj1 (record_head_none l lvl) H r Hr Hl).
    - intros st rank v lvl pers. unfold after, produce. rewrite <- (bumped_size st).
      apply with_settles_size. intros f s. unfold fresh_offer.
      destruct (record_head (s_records s) _); [unfold size; cbn; lia | apply offer_loop_size].
    - intros st rank pat g lane pers peek. unfold after, consume. rewrite <- (bumped_size st).
      apply with_settles_size. intros f s. unfold fresh_receive.
      destruct (waiter_head (s_waiters s) _); [unfold size; cbn; lia | apply receive_loop_size].
    - intros st. unfold after, settle_job, settle_of. cbn [o_ops].
      pose proof (settle_size settle_fuel 0 (bumped st) None) as H. cbn [flight_measure] in H.
      rewrite bumped_size in H. lia.
  Qed.

  (* ── 18. The Unsafe control: a priority queue without the settle ─────── *)

  (* The plans without the settles before and after them (the Unsafe control
     PqNoSettleUnsafe of PriorityQueue.tla). *)
  Definition produce_without_settle (st : sstate) (rank : nat) (v : V) (lvl : option nat)
    (pers : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    let p := fresh_offer (mkRecord o rank lvl v pers) 0 (bumped st) in
    mkOutcome (p_ops p)
      (match p_end p with PDone => None | PReissue c => Some (NextOperation c (carried pers o)) end).

  Definition consume_without_settle (st : sstate) (rank : nat) (pat : P) (g : G)
    (lane : option nat) (pers peek : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    let p := fresh_receive (mkWaiter o rank lane pat g pers peek) 0 (bumped st) in
    mkOutcome (p_ops p)
      (match p_end p with PDone => None | PReissue c => Some (NextOperation c (carried pers o)) end).

  (* INV as a computation, for the counterexamples. *)
  Definition no_pending_matchb (st : sstate) : bool :=
    forallb (fun w => negb (is_waiter_head (s_waiters st) w && has (fireable (s_records st) w)))
      (s_waiters st).

  Lemma no_pending_matchb_iff : forall st, no_pending_matchb st = true <-> no_pending_match st.
  Proof.
    intros st. unfold no_pending_matchb, no_pending_match. rewrite forallb_forall. split.
    - intros H w Hw Hh. specialize (H w Hw). rewrite Hh in H. cbn [andb negb] in H.
      destruct (fireable (s_records st) w); [cbn in H; discriminate | reflexivity].
    - intros H w Hw. destruct (is_waiter_head (s_waiters st) w) eqn:Hh; [| reflexivity].
      rewrite (H w Hw Hh). reflexivity.
  Qed.
End Pq.

(* ── 18a. Headline 13: the settle is needed ─────────────────────────────── *)

(* The priority queue of the counterexamples. Payloads and patterns are
   numbers, and a pattern matches a payload when they are equal. Every guard
   admits, and the cap is 64 (FIRE_ROUND_CAP). *)
Definition ce_admits (g : unit) (v : nat) : bool := true.
Definition ce_produce := produce nat nat unit Nat.eqb ce_admits 64.
Definition ce_consume := consume nat nat unit Nat.eqb ce_admits 64.
Definition ce_produce_without_settle := produce_without_settle nat nat unit Nat.eqb ce_admits 64.
Definition ce_consume_without_settle := consume_without_settle nat nat unit Nat.eqb ce_admits 64.
Definition ce_after := after nat nat unit.
Definition ce_inv := no_pending_match nat nat unit Nat.eqb ce_admits.
Definition ce_empty : sstate nat nat unit := mkS nat nat unit [] [] None.

(* Counterexample A (a fire exposes a waiter). The receive wA1 for 1 parks
   in the unrestricted queue (ord 0), and the receive wA2 for 2 parks behind
   it (ord 1). The send r1 of 2 at the level 1 meets the head wA1, which
   refuses it, so r1 is stored (ord 2). The send d of 1 at the level 0 then
   fires wA1 and exposes wA2, which matches r1. *)
Definition ce_wA1 : waiter nat unit := mkWaiter nat unit 0 0 None 1 tt false false.
Definition ce_wA2 : waiter nat unit := mkWaiter nat unit 1 0 None 2 tt false false.
Definition ce_r1 : record nat := mkRecord nat 2 0 (Some 1) 2 false.
Definition ce_d : record nat := mkRecord nat 3 0 (Some 0) 1 false.
Definition ce_state_a : sstate nat nat unit := mkS nat nat unit [ce_r1] [ce_wA2; ce_wA1] (Some 3).

(* Counterexample B (a fire exposes a record). The receive wL for 2 parks in
   the waiter queue of the level 0 (ord 0). The sends h1 of 1 and h2 of 2 at
   the level 0 are stored (ords 1 and 2). The head wL refuses the head h1, and
   h2 waits behind h1. The unrestricted receive k for 1 then takes h1 and
   exposes h2, which wL matches. *)
Definition ce_wL : waiter nat unit := mkWaiter nat unit 0 0 (Some 0) 2 tt false false.
Definition ce_h1 : record nat := mkRecord nat 1 0 (Some 0) 1 false.
Definition ce_h2 : record nat := mkRecord nat 2 0 (Some 0) 2 false.
Definition ce_k : waiter nat unit := mkWaiter nat unit 3 0 None 1 tt false false.
Definition ce_state_b : sstate nat nat unit := mkS nat nat unit [ce_h2; ce_h1] [ce_wL] (Some 3).

(* Headline 13. A priority queue without the settle breaks INV, in both
   directions of exposure. The operations reach ce_state_a and ce_state_b,
   which satisfy INV. Without the settle, the next operation fires a head and
   ends with a waiter-lane head that can fire. So INV fails at the end of a
   completed operation. With the settle after the plan, the priority queue
   lifts the exposed record and offers it again to the oldest fireable head.
   Then INV holds. *)
Theorem pq_without_settle_breaks_inv :
  (let s1 := ce_after ce_empty (ce_consume ce_empty 0 1 tt None false false) in
   let s2 := ce_after s1 (ce_consume s1 0 2 tt None false false) in
   ce_after s2 (ce_produce s2 0 2 (Some 1) false) = ce_state_a) /\
  ce_inv ce_state_a /\
  ce_produce_without_settle ce_state_a 0 1 (Some 0) false =
    mkOutcome nat nat unit [SFireProduce nat nat unit ce_wA1 ce_d] None /\
  ~ ce_inv (ce_after ce_state_a (ce_produce_without_settle ce_state_a 0 1 (Some 0) false)) /\
  ce_produce ce_state_a 0 1 (Some 0) false =
    mkOutcome nat nat unit
      [SFireProduce nat nat unit ce_wA1 ce_d; SLift nat nat unit ce_r1;
       SFireProduce nat nat unit ce_wA2 ce_r1] None /\
  ce_inv (ce_after ce_state_a (ce_produce ce_state_a 0 1 (Some 0) false)) /\
  (let s1 := ce_after ce_empty (ce_consume ce_empty 0 2 tt (Some 0) false false) in
   let s2 := ce_after s1 (ce_produce s1 0 1 (Some 0) false) in
   ce_after s2 (ce_produce s2 0 2 (Some 0) false) = ce_state_b) /\
  ce_inv ce_state_b /\
  ce_consume_without_settle ce_state_b 0 1 tt None false false =
    mkOutcome nat nat unit [SFireConsume nat nat unit ce_k ce_h1] None /\
  ~ ce_inv (ce_after ce_state_b (ce_consume_without_settle ce_state_b 0 1 tt None false false)) /\
  ce_consume ce_state_b 0 1 tt None false false =
    mkOutcome nat nat unit
      [SFireConsume nat nat unit ce_k ce_h1; SLift nat nat unit ce_h2;
       SFireProduce nat nat unit ce_wL ce_h2] None /\
  ce_inv (ce_after ce_state_b (ce_consume ce_state_b 0 1 tt None false false)).
Proof.
  unfold ce_inv.
  split; [reflexivity |].
  split; [apply no_pending_matchb_iff; reflexivity |].
  split; [reflexivity |].
  split; [intros H; apply no_pending_matchb_iff in H; vm_compute in H; discriminate H |].
  split; [reflexivity |].
  split; [apply no_pending_matchb_iff; reflexivity |].
  split; [reflexivity |].
  split; [apply no_pending_matchb_iff; reflexivity |].
  split; [reflexivity |].
  split; [intros H; apply no_pending_matchb_iff in H; vm_compute in H; discriminate H |].
  split; [reflexivity |].
  apply no_pending_matchb_iff; reflexivity.
Qed.

(* ── 18b. Headline 7: the order of the settle matters ──────────────────── *)

(* The example of the readiness review S4. The unrestricted queue holds U1 (ord
   0) and U2 (ord 1). The waiter queue of the level 1 holds R (ord 2). The
   levels 0 and 1 hold the heads h0 (ord 3) and h1 (ord 4). U1 matches only h0,
   and U2 and R match h1. Such a view is owed a settle, for example after a
   plan that exposed it. *)
Definition so_U1 : waiter nat unit := mkWaiter nat unit 0 0 None 0 tt false false.
Definition so_U2 : waiter nat unit := mkWaiter nat unit 1 0 None 1 tt false false.
Definition so_R : waiter nat unit := mkWaiter nat unit 2 0 (Some 1) 1 tt false false.
Definition so_h0 : record nat := mkRecord nat 3 0 (Some 0) 0 false.
Definition so_h1 : record nat := mkRecord nat 4 0 (Some 1) 1 false.
Definition so_state : sstate nat nat unit :=
  mkS nat nat unit [so_h0; so_h1] [so_U1; so_U2; so_R] (Some 5).

(* Order A fires the head R of the level queue first. Order B fires the
   oldest fireable head first: U1, and then U2, which the fire of U1 makes
   the head of the unrestricted queue. *)
Definition so_ops_a : list (staged nat nat unit) :=
  [SLift nat nat unit so_h1; SFireProduce nat nat unit so_R so_h1;
   SLift nat nat unit so_h0; SFireProduce nat nat unit so_U1 so_h0].
Definition so_ops_b : list (staged nat nat unit) :=
  [SLift nat nat unit so_h0; SFireProduce nat nat unit so_U1 so_h0;
   SLift nat nat unit so_h1; SFireProduce nat nat unit so_U2 so_h1].

Definition ce_run := run nat nat unit.
Definition ce_fireable := fireable nat nat unit Nat.eqb ce_admits.
Definition ce_is_waiter_head := is_waiter_head nat unit.
Definition ce_settle_at := settle_at nat nat unit Nat.eqb ce_admits 64.

(* Headline 7. The order of the settle matters (readiness review S4):
   strict confluence fails for a priority queue. In so_state, the head R of
   the waiter queue of the level 1 and the head U1 of the unrestricted queue
   can both fire. Order A fires R first and then U1, and leaves U2 parked.
   Order B fires U1 first, which makes U2 the head of the unrestricted
   queue, and the older U2 then wins h1, which leaves R parked. Both ends
   satisfy INV, and they differ. So the settle fixes the order: the oldest
   fireable head fires first, and the settle of so_state stages order B. *)
Theorem pq_settle_order_matters :
  ce_is_waiter_head (s_waiters nat nat unit so_state) so_R = true /\
  ce_fireable (s_records nat nat unit so_state) so_R = Some so_h1 /\
  ce_is_waiter_head (s_waiters nat nat unit so_state) so_U1 = true /\
  ce_fireable (s_records nat nat unit so_state) so_U1 = Some so_h0 /\
  ce_run so_state so_ops_a = mkS nat nat unit [] [so_U2] (Some 5) /\
  ce_run so_state so_ops_b = mkS nat nat unit [] [so_R] (Some 5) /\
  ce_run so_state so_ops_a <> ce_run so_state so_ops_b /\
  ce_inv (ce_run so_state so_ops_a) /\
  ce_inv (ce_run so_state so_ops_b) /\
  ce_settle_at 0 so_state = mkSettled nat nat unit so_ops_b Quiescent.
Proof.
  unfold ce_inv.
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [vm_compute; discriminate |].
  split; [apply no_pending_matchb_iff; reflexivity |].
  split; [apply no_pending_matchb_iff; reflexivity |].
  reflexivity.
Qed.

(* ── 19. Headline 19: the level scans reserve their reads first ─────────── *)

(* The metering rule of DR-88 (decision 42), restated from AgentReadCoverage.v
   so that this module stays standalone. A metered computation is a list of
   events. Reserve u adds u units to the host-work budget. Read u is work of u
   units that no other step meters. Prefix coverage: every prefix reads at most
   the units that the same prefix has reserved. Each host-work dimension
   satisfies the rule alone, so the model counts one. *)
Inductive event : Type := Reserve (units : nat) | Read (units : nat).

Definition reserved_units (e : event) : nat :=
  match e with Reserve units => units | Read _ => 0 end.

Definition read_units (e : event) : nat :=
  match e with Read units => units | Reserve _ => 0 end.

Definition total (project : event -> nat) (trace : list event) : nat :=
  fold_right Nat.add 0 (map project trace).

Lemma total_nil : forall project, total project [] = 0.
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
  - exact (covered_left prefix middle left_prefix).
  - subst prefix.
    pose proof (covered_right middle suffix right_middle) as inner.
    rewrite !total_app. lia.
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

Lemma sum_cons : forall amount rest, sum (amount :: rest) = amount + sum rest.
Proof. reflexivity. Qed.

Arguments sum : simpl never.

Lemma steps_reserved : forall amounts, total reserved_units (flat_map step amounts) = sum amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  change (flat_map step (amount :: rest)) with (step amount ++ flat_map step rest).
  rewrite total_app, IH, sum_cons. unfold total, step. simpl. lia.
Qed.

Lemma steps_read : forall amounts, total read_units (flat_map step amounts) = sum amounts.
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

Lemma step_balanced : forall units, balanced (step units).
Proof. intros units. exact (steps_balanced [units]). Qed.

Lemma balanced_nil : balanced [].
Proof. split; [intros slack; apply covered_from_nil | reflexivity]. Qed.

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

Lemma balanced_flat_map : forall (A : Type) (f : A -> list event) l,
  (forall a, balanced (f a)) -> balanced (flat_map f l).
Proof.
  intros A f l H. induction l as [| a rest IH]; [exact balanced_nil |].
  change (balanced (f a ++ flat_map f rest)). apply balanced_app; [apply H | exact IH].
Qed.

Lemma reserve_covered : forall slack units, covered_from slack [Reserve units].
Proof.
  intros slack units prefix suffix split.
  destruct prefix as [| a [| b rest]]; simpl in split.
  - rewrite total_nil. lia.
  - injection split as ha _. subst a. unfold total. simpl. lia.
  - discriminate.
Qed.

(* A list of reads is covered from a slack of at least its units. *)
Lemma reads_only_covered : forall slack trace,
  total reserved_units trace = 0 -> total read_units trace <= slack -> covered_from slack trace.
Proof.
  intros slack trace no_reserve bound prefix suffix split.
  subst trace. rewrite total_app in bound. lia.
Qed.

(* An entry of one side of a collection in the staged view, as
   PriorityQueueDiscipline::lane_heads scans it. The scan decodes its envelope
   (its order and its lane). It looks up the lane of the entry in the map of
   heads. Then it grows the map, or it compares the entry with the head of its
   lane (head_metered on the pair). *)
Record entry : Type := mkEntry { envelope : nat; lookup : nat; placed : nat }.

(* The test of one waiter-lane head in PriorityQueueView::next_fire. The test
   copies the waiter and decodes the payloads of the level heads that its lane
   sees. A round decodes each level head at most once. Then the test has the
   reads of the metered matcher and of the guard, and the comparison with the
   best fire so far. *)
Record head_test : Type :=
  mkTest { copy_waiter : nat; decodes : list nat; matches : list nat; guard : nat; versus : nat }.

Section LevelScanMetering.
  (* The units of a buffer of n slots. *)
  Variable buffer : nat -> nat.

  (* PriorityQueueDiscipline::lane_heads over one side. *)
  Definition lane_scan (side : list entry) : list event :=
    flat_map (fun e => step (envelope e) ++ step (lookup e) ++ step (placed e)) side.

  Definition test_events (t : head_test) : list event :=
    step (copy_waiter t) ++ flat_map step (decodes t) ++ flat_map step (matches t)
    ++ step (guard t) ++ step (versus t).

  (* One round of the settle loop that fires (settle.rs and
     PriorityQueueView). *)
  Record settle_round : Type := mkRound {
    flight_decode : nat;        (* the decode of the slots of the record in flight, or 0 *)
    record_side : list entry;   (* the records that lane_heads scans *)
    level_heads : nat;          (* the slots of the buffers of the level heads and payloads *)
    flight_compare : nat;       (* the comparison of the record in flight with its level head *)
    waiter_side : list entry;   (* the waiters that lane_heads scans *)
    tests : list head_test;     (* the waiter-lane heads, in lane order *)
    bound_copy : nat;           (* the copy of a stored bound record, 0 for the record in flight *)
    plan_copies : list nat;     (* the buffers of the plan and the copy of the channel *)
    lift : list nat;            (* the match, template, random bytes, copies and buffers of K_lift *)
    unwrap : nat;               (* the decode of the fired continuation, which the round prepays *)
    reoffer : list nat          (* the introduction and the copies of the re-offer *)
  }.

  Definition round_staged (r : settle_round) : list event :=
    step (flight_decode r)
    ++ lane_scan (record_side r)
    ++ step (buffer (level_heads r))
    ++ step (flight_compare r)
    ++ lane_scan (waiter_side r)
    ++ step (buffer (level_heads r))
    ++ flat_map test_events (tests r)
    ++ step (bound_copy r)
    ++ flat_map step (plan_copies r)
    ++ flat_map step (lift r)
    ++ [Reserve (unwrap r)]
    ++ flat_map step (reoffer r).

  (* One round of the loop of receive_pq that fires. The round scans the lanes
     of the records and fills the buffer of record_head_list and the buffer of
     the payloads. It decodes payloads and matches them in first_match, and it
     checks the guard. It copies the bound record and prepays the decode of the
     continuation. Then it copies for the re-store of a peek and for the fire. *)
  Record receive_round : Type := mkReceive {
    rr_records : list entry; rr_heads : nat; rr_decodes : list nat; rr_matches : list nat;
    rr_guard : nat; rr_head_copy : nat; rr_unwrap : nat; rr_fire : list nat }.

  Definition receive_staged (r : receive_round) : list event :=
    lane_scan (rr_records r) ++ step (buffer (rr_heads r)) ++ step (buffer (rr_heads r))
    ++ flat_map step (rr_decodes r) ++ flat_map step (rr_matches r) ++ step (rr_guard r)
    ++ step (rr_head_copy r) ++ [Reserve (rr_unwrap r)] ++ flat_map step (rr_fire r).

  (* One round of the loop of offer_pq that fires. The round scans the lanes of
     the waiters. It looks up the candidate lanes and compares their orders
     (offer_target). It copies, matches and guards each tested candidate. Then
     it prepays the decode of the continuation and copies for the fire. *)
  Record offer_round : Type := mkOffer {
    or_waiters : list entry; or_lookups : list nat; or_compare : nat; or_tests : list nat;
    or_unwrap : nat; or_fire : list nat }.

  Definition offer_staged (r : offer_round) : list event :=
    lane_scan (or_waiters r) ++ flat_map step (or_lookups r) ++ step (or_compare r)
    ++ flat_map step (or_tests r) ++ [Reserve (or_unwrap r)] ++ flat_map step (or_fire r).

  (* A round of a transaction. It is a settle round that fires, or a round of
     the receive plan or of the produce plan that fires. It can also be a
     self-metered round with no fire. Such a round lands the record in flight
     before a fire of a stored record (Cx::restore_on), and plans again. *)
  Inductive loop_round : Type :=
  | FireRound (r : settle_round)
  | ReceiveRound (r : receive_round)
  | OfferRound (r : offer_round)
  | LandRound (work : list nat).

  Definition loop_staged (l : loop_round) : list event :=
    match l with
    | FireRound r => round_staged r
    | ReceiveRound r => receive_staged r
    | OfferRound r => offer_staged r
    | LandRound work => flat_map step work
    end.

  Definition prepaid (l : loop_round) : nat :=
    match l with
    | FireRound r => unwrap r
    | ReceiveRound r => rr_unwrap r
    | OfferRound r => or_unwrap r
    | LandRound _ => 0
    end.

  (* After the commit, the outcome decodes each fired continuation. *)
  Definition loop_after_commit (l : loop_round) : list event :=
    match l with LandRound _ => [] | _ => [Read (prepaid l)] end.

  (* A transaction: its rounds, then its last checks. The last checks are
     self-metered. They are the scans and the matches that find no fire, and
     the cap checks. They also land a record in flight, and they store or park
     the element. *)
  Record txn_run : Type := mkRun { rounds : list loop_round; last_check : list nat }.

  Definition txn_staged (x : txn_run) : list event :=
    flat_map loop_staged (rounds x) ++ flat_map step (last_check x).

  Definition txn_after_commit (x : txn_run) : list event :=
    flat_map loop_after_commit (rounds x).

  Definition txn_trace (x : txn_run) : list event := txn_staged x ++ txn_after_commit x.

  Lemma lane_scan_balanced : forall side, balanced (lane_scan side).
  Proof.
    intros side. unfold lane_scan. apply balanced_flat_map. intros e.
    repeat apply balanced_app; apply step_balanced.
  Qed.

  Lemma test_balanced : forall t, balanced (test_events t).
  Proof.
    intros t. unfold test_events.
    repeat apply balanced_app; first [apply step_balanced | apply steps_balanced].
  Qed.

  (* The parts of a settle round other than the prepaid reservation. *)
  Lemma round_parts_balanced : forall r,
    balanced (step (flight_decode r) ++ lane_scan (record_side r) ++ step (buffer (level_heads r))
              ++ step (flight_compare r) ++ lane_scan (waiter_side r)
              ++ step (buffer (level_heads r)) ++ flat_map test_events (tests r)
              ++ step (bound_copy r) ++ flat_map step (plan_copies r) ++ flat_map step (lift r)).
  Proof.
    intros r.
    repeat apply balanced_app;
      first [ apply step_balanced | apply steps_balanced | apply lane_scan_balanced
            | apply balanced_flat_map; apply test_balanced ].
  Qed.

  Lemma round_staged_shape : forall r,
    round_staged r =
      (step (flight_decode r) ++ lane_scan (record_side r) ++ step (buffer (level_heads r))
       ++ step (flight_compare r) ++ lane_scan (waiter_side r)
       ++ step (buffer (level_heads r)) ++ flat_map test_events (tests r)
       ++ step (bound_copy r) ++ flat_map step (plan_copies r) ++ flat_map step (lift r))
      ++ [Reserve (unwrap r)] ++ flat_map step (reoffer r).
  Proof. intros r. unfold round_staged. rewrite <- !app_assoc. reflexivity. Qed.

  Lemma receive_staged_shape : forall r,
    receive_staged r =
      (lane_scan (rr_records r) ++ step (buffer (rr_heads r)) ++ step (buffer (rr_heads r))
       ++ flat_map step (rr_decodes r) ++ flat_map step (rr_matches r) ++ step (rr_guard r)
       ++ step (rr_head_copy r))
      ++ [Reserve (rr_unwrap r)] ++ flat_map step (rr_fire r).
  Proof. intros r. unfold receive_staged. rewrite <- !app_assoc. reflexivity. Qed.

  Lemma offer_staged_shape : forall r,
    offer_staged r =
      (lane_scan (or_waiters r) ++ flat_map step (or_lookups r) ++ step (or_compare r)
       ++ flat_map step (or_tests r))
      ++ [Reserve (or_unwrap r)] ++ flat_map step (or_fire r).
  Proof. intros r. unfold offer_staged. rewrite <- !app_assoc. reflexivity. Qed.

  (* A reservation between two balanced segments leaves its units as slack. *)
  Lemma totals_prepaid : forall parts u rest,
    balanced parts -> balanced rest ->
    total read_units (parts ++ [Reserve u] ++ rest) + u =
    total reserved_units (parts ++ [Reserve u] ++ rest).
  Proof.
    intros parts u rest [_ Hp] [_ Hr]. rewrite !total_app, Hp, Hr.
    cbn [total map fold_right read_units reserved_units]. lia.
  Qed.

  Lemma prepaid_covered : forall slack parts u rest,
    balanced parts -> balanced rest -> covered_from slack (parts ++ [Reserve u] ++ rest).
  Proof.
    intros slack parts u rest [parts_covered parts_equal] [rest_covered _].
    apply covered_from_app; [apply parts_covered | lia |].
    apply covered_from_app; [apply reserve_covered | unfold total; simpl; lia |].
    apply rest_covered.
  Qed.

  Lemma receive_parts_balanced : forall r,
    balanced (lane_scan (rr_records r) ++ step (buffer (rr_heads r)) ++ step (buffer (rr_heads r))
              ++ flat_map step (rr_decodes r) ++ flat_map step (rr_matches r) ++ step (rr_guard r)
              ++ step (rr_head_copy r)).
  Proof.
    intros r. repeat apply balanced_app;
      first [apply step_balanced | apply steps_balanced | apply lane_scan_balanced].
  Qed.

  Lemma offer_parts_balanced : forall r,
    balanced (lane_scan (or_waiters r) ++ flat_map step (or_lookups r) ++ step (or_compare r)
              ++ flat_map step (or_tests r)).
  Proof.
    intros r. repeat apply balanced_app;
      first [apply step_balanced | apply steps_balanced | apply lane_scan_balanced].
  Qed.

  (* A staged round is covered, and it leaves its prepaid units as slack. *)
  Lemma loop_staged_covered : forall slack l, covered_from slack (loop_staged l).
  Proof.
    intros slack [r | r | r | work]; cbn [loop_staged].
    - rewrite round_staged_shape.
      apply prepaid_covered; [apply round_parts_balanced | apply steps_balanced].
    - rewrite receive_staged_shape.
      apply prepaid_covered; [apply receive_parts_balanced | apply steps_balanced].
    - rewrite offer_staged_shape.
      apply prepaid_covered; [apply offer_parts_balanced | apply steps_balanced].
    - apply steps_covered.
  Qed.

  Lemma loop_staged_totals : forall l,
    total read_units (loop_staged l) + prepaid l = total reserved_units (loop_staged l).
  Proof.
    intros [r | r | r | work]; cbn [loop_staged prepaid].
    - rewrite round_staged_shape.
      apply totals_prepaid; [apply round_parts_balanced | apply steps_balanced].
    - rewrite receive_staged_shape.
      apply totals_prepaid; [apply receive_parts_balanced | apply steps_balanced].
    - rewrite offer_staged_shape.
      apply totals_prepaid; [apply offer_parts_balanced | apply steps_balanced].
    - rewrite steps_read, steps_reserved. lia.
  Qed.

  Lemma rounds_staged_covered : forall slack ls, covered_from slack (flat_map loop_staged ls).
  Proof.
    intros slack ls. revert slack.
    induction ls as [| l rest IH]; intros slack; [apply covered_from_nil |].
    change (covered_from slack (loop_staged l ++ flat_map loop_staged rest)).
    pose proof (loop_staged_totals l) as totals.
    apply covered_from_app; [apply loop_staged_covered | lia | apply IH].
  Qed.

  Lemma rounds_staged_totals : forall ls,
    total read_units (flat_map loop_staged ls) + sum (map prepaid ls) =
    total reserved_units (flat_map loop_staged ls).
  Proof.
    induction ls as [| l rest IH]; [reflexivity |].
    change (flat_map loop_staged (l :: rest)) with (loop_staged l ++ flat_map loop_staged rest).
    pose proof (loop_staged_totals l).
    cbn [map]. rewrite !total_app, sum_cons. lia.
  Qed.

  Lemma after_rounds_read : forall ls,
    total read_units (flat_map loop_after_commit ls) = sum (map prepaid ls).
  Proof.
    induction ls as [| l rest IH]; [reflexivity |].
    change (flat_map loop_after_commit (l :: rest))
      with (loop_after_commit l ++ flat_map loop_after_commit rest).
    cbn [map]. rewrite total_app, IH, sum_cons.
    destruct l as [r | r | r | work]; unfold loop_after_commit, total; simpl; lia.
  Qed.

  Lemma after_rounds_reserved : forall ls,
    total reserved_units (flat_map loop_after_commit ls) = 0.
  Proof.
    induction ls as [| l rest IH]; [reflexivity |].
    change (flat_map loop_after_commit (l :: rest))
      with (loop_after_commit l ++ flat_map loop_after_commit rest).
    rewrite total_app, IH.
    destruct l as [r | r | r | work]; unfold loop_after_commit, total; simpl; lia.
  Qed.

  (* Headline 19. The level scans of a priority queue reserve their reads
     before they make them (decision 42, DR-88). The trace of a transaction
     satisfies prefix coverage. This includes the reads of the outcome after
     the commit. Each settle round decodes the slots of the record in flight,
     scans the lanes of both sides and builds the list of level heads. It tests
     each waiter-lane head (copy, payload decodes, matches, guard, comparison).
     Then it copies the bound record and the plan, lifts, prepays the decode of
     the fired continuation and stages the re-offer. Each round of the receive
     plan scans the level heads, matches, checks the guard, copies the head,
     prepays and fires. Each round of the produce plan scans the waiter lanes,
     tests the candidates, prepays and fires. Each step is self-metered, and a
     landing round is self-metered. *)
  Theorem level_scan_trace_covered : forall x, covered (txn_trace x).
  Proof.
    intros x. unfold covered, txn_trace, txn_staged.
    pose proof (rounds_staged_totals (rounds x)) as Hrounds.
    pose proof (steps_balanced (last_check x)) as [Hlast_cov Hlast_eq].
    rewrite <- app_assoc.
    apply covered_from_app; [apply rounds_staged_covered | lia |].
    apply covered_from_app; [apply Hlast_cov | lia |].
    apply reads_only_covered; unfold txn_after_commit.
    - apply after_rounds_reserved.
    - rewrite after_rounds_read. lia.
  Qed.
End LevelScanMetering.

Print Assumptions pq_level_order.
Print Assumptions pq_unprioritized_lowest.
Print Assumptions pq_fifo_within_level.
Print Assumptions pq_restricted_waiter_in_level_queue.
Print Assumptions pq_unrestricted_first_matching_level.
Print Assumptions pq_settle_oldest_first.
Print Assumptions pq_settle_order_matters.
Print Assumptions pq_produce_preserves_inv.
Print Assumptions pq_consume_preserves_inv.
Print Assumptions pq_head_blocks_within_level.
Print Assumptions pq_settle_terminates.
Print Assumptions pq_plan_permutation_invariant.
Print Assumptions pq_without_settle_breaks_inv.
Print Assumptions pq_priority_domain.
Print Assumptions pq_no_level_storage.
Print Assumptions pq_reissue_takes_level_fifo_place.
Print Assumptions pq_reentered_waiter_keeps_seniority.
Print Assumptions pq_peek_restores_head.
Print Assumptions level_scan_trace_covered.
Print Assumptions pq_settle_keeps_unspent_records.
