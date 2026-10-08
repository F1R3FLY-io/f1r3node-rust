(* ═══════════════════════════════════════════════════════════════════════════
   StackDiscipline.v — head-only matching and the settle of the stack (P5b)

   This module models the stack discipline of the native spaces (plan
   decision 11) and the settle of a collection (F2 of decision 39, the
   P5 design v3, section A.8). A stack is head-only on both sides. The top
   of a side is its entry with the greatest (ord, bincode bytes), and only
   the two tops interact. A top that does not match blocks the entries
   below it. Every operation bumps the collection counter (decision 31), so
   a fresh element has the greatest order of its side and always becomes
   its top. Records and waiters therefore leave in LIFO order.

   A fresh element on top of its side can fire the top of the other side
   and so expose an older pair below the tops. The stack then settles: it
   fires the top pair while the two tops match. Both entries of such a pair
   are stored, so the settle lifts the record out of the store (a consume
   of K_lift) and offers it again to the waiter with its own identity. A
   persistent record is not lifted, and its re-offer leaves it stored. A
   record that a waiter that peeks did not spend stays in flight (decision
   30), and the settle stores it again at its order when it has no further
   fire (decision 45, Rule E).

   The plans of an operation (offer_head and receive_head of generic.rs
   with the hooks of StackDiscipline). One space transaction makes at most
   cap fires, its settles included (revision I-21). So one fire count runs
   through the three phases of the transaction (Cx::rounds):
     settle before the plan            starts the count at 0. A cap or a
                                       divergent pair re-issues the
                                       operation (decision 43 order)
     open_plan                         when the settle before the plan ends
                                       at the cap, the plan stages nothing
                                       and the operation re-issues
                                       (RoundCap, decision 43 order)
     at the top: always for a fresh element. A re-issued persistent element
       is at the top when its side is empty or its order is greater than
       the order of the top (precedes is >), else it is stored or parked at
       its LIFO place (decision 43)
     the loop of the plan: continue the count. Fire the top of the other
       side while it fits, and store or park the element at the end. A peek
       of a record that is not persistent stores the record again at once
       (Rule E)
     settle after the plan             continues the count of the plan. A
                                       cap or a divergent pair re-issues a
                                       settle job
   A settle job (SpaceOp::Settle) is a transaction of its own. Its settle
   starts the count at 0.
   The settle loop (settle.rs):
     loop: F := the top waiter and the top record (or the record in flight)
           when the waiter fits the record, else land the record in flight
           and stop (Quiescent)
           at cap fires of the transaction: land the record in flight and
           re-issue
           lift the record when it is stored and not persistent
           re-offer the record to the waiter: after_fire decides
           Done or Continue: go on (a record that a peek left stays in
           flight), Divergent or RoundCap: land and re-issue

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md, sections A.3.3, A.8
     (Settle), A.12 (the stack row) and A.13 (the P5b row); p5-design-v2.md,
     section 3.1 (the stack, the settle, the counterexample of headline 8,
     and the termination measure); the decision 43 design, section 43.8
     (the stack re-enters at its LIFO place); the decision 45 design report,
     section C.11 (a peek re-stores before any settle step).
   - Plan: decisions 11 (head-only), 30 (the re-offer of a datum that a
     peek waiter left), 31 (every operation bumps the counter), 37 (the
     current guard rule: a head-only collection tests only its top), 42
     (reserve every read and copy), 43 (a re-issued persistent operation
     keeps its first-round order), 45 (Rule E), revision I-21
     (FIRE_ROUND_CAP: one space transaction makes at most FIRE_ROUND_CAP
     fires, its settles included), and the P5 choice F2 (stack and
     priority queue settle with lift and re-offer).
   - Code: the STACK batch of wave W2 of P5 (spaces/settle.rs and
     spaces/disciplines/stack.rs, on the spine S1 commit 21b2b27fd), with
     the I-21 change: one fire count for each transaction (Plan.rounds,
     Cx::rounds and open_plan of generic.rs). The line numbers below are
     the line numbers after that change.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (rholang/src/rust/interpreter)
   ─────────────────────────────────────────────────────────────────────────
   above, top_from, top      spaces/disciplines/stack.rs:75-93, 227-262
                             (StackDiscipline::head_metered: the greatest
                             (ord, bytes), the earlier index on equal keys,
                             and Top::challenge)
   reenter_offer,            spaces/disciplines/stack.rs:96, 99-108
   reenter_receive           (reaches_head is true, precedes is ord > head)
                             and spaces/generic.rs:918-939 (Cx::at_head)
   after_fire, cap           reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,
                             FireRounds::after_fire)
   next_fire                 spaces/disciplines/stack.rs:159-212
                             (StackView::next_fire: the top waiter, then
                             the top record or the record in flight, then
                             Cx::matches)
   settle_loop, lift_ops,    spaces/settle.rs:404-498, 508-513 and 519-563
   flying, flight_after,     (settle, land and lift)
   land
   at_cap                    spaces/settle.rs:431 (the cap check of the
                             transaction at the top of a settle round)
   settle_of, settle_at      spaces/settle.rs:569-582, 588-601 and 606-616
                             (settle_before_plan, settle_after_plan and
                             settle_job: one settle call each). settle_of
                             starts the count at 0 (the settle before the
                             plan and a settle job). settle_at continues
                             the count (the settle after the plan).
   fires (the fire count)    spaces/generic.rs:685, 746 and 2344
                             (Plan.rounds, Cx::rounds, and the new
                             FireRounds of each transaction in run_with)
   plan_at_cap               spaces/generic.rs:1503-1516 (open_plan: the
                             settle before the plan, then the cap check of
                             the transaction)
   offer_loop                spaces/generic.rs:1524-1591 (offer_head)
   receive_loop,             spaces/generic.rs:1600-1684 and 1687-1696
   consume_fire              (receive_head and restore_peeked: the plain
                             re-store of a peeked record, Rule E)
   with_settles              spaces/generic.rs:1503-1516, 1524-1591 and
                             1600-1684 (open_plan and the settle hooks
                             around the plans) and
                             spaces/disciplines/stack.rs:112-138
   carried, next_round       spaces/settle.rs:643-675 and 681-705
                             (reissued_operation: persistent.then(first-
                             round order), and settle_operation) and
                             spaces/generic.rs:1583, 1676 (the re-issue of
                             a plan)
   produce, consume,         spaces/generic.rs:2288-2424 (run_with of
   bumped, bump_ord          FamilySpace: the bump, then the plan) and
                             spaces/counter.rs:165-245 (bump)
   apply, run                the staged view of the substrate
                             (space_store_handle.rs, stage_produce and
                             stage_consume): a store puts its record in
                             front, a fire removes its non-persistent
                             partners, a produce-side fire never stores its
                             incoming record
   drop_key                  the removal of a record or a waiter by its
                             identity (claim_positions and remove_records
                             of space_store_handle.rs)
   produce_without_settle    the Unsafe control StackNoSettle of
                             StackDiscipline.tla (no settle hooks)
   scan, settle_round,       spaces/disciplines/stack.rs:159-212,
   settle_trace              spaces/settle.rs:404-498 and 519-563, and
                             spaces/generic.rs:849-877 (the scans of the
                             two tops, the payload decode, the metered
                             match, the copies of the plan and of the
                             lift, the prepaid unwrap, the re-offer)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - fits p v is the metered spatial match of the payload against the
     pattern followed by the guard (Cx::matches). It is a section variable,
     so every theorem holds for every matcher. A head-only stack tests only
     its tops, so the guard rule of decision 37 is the same on both sides.
   - cap is FIRE_ROUND_CAP. It is a section variable, so every theorem holds
     for every cap. Headline 8 instantiates 64.
   - rank is the position of the bincode bytes of an entry in their order.
     Two entries with one (ord, rank) are byte-identical.
   - The staged view of a collection is a record list and a waiter list. The
     state after staged operations is the fold of apply over them (run).
     The bump changes the counter only (bumped), as the counter channel is
     not part of the view of the collection.
   - A removal takes the first entry with the key of the removed entry
     (drop_key). With distinct orders (wf), the key names the entry.
   - The model omits the Inconsistent errors of operation_ord and of
     Cx::at_head. A re-entered element with the order of the top is stored
     at its place here. wf' of decision 43 (distinct orders, the element
     included) excludes equal orders, and headline 9 assumes it.
   - A settle fire binds one record on one channel, as the engine stages a
     produce-side fire over one channel (Cx::fire_offer_on).
   - The identities of K_lift are not modeled. The step of a K_lift is the
     fire count of the transaction at the lift (spaces/settle.rs:450 and
     501-503, lift_step). The Rust tests
     space_settle_lift_identity_is_unique_per_counter_and_step and
     space_stack_two_settles_of_one_round_lift_with_distinct_steps pin them.
   - One fire count runs through each transaction, as in the code (I-21).
     The model counts the fire operations that the transaction staged so
     far (fires). Each fire stages exactly one fire operation, so this
     count is the count of Cx::rounds. The settle before the plan and a
     settle job start at 0. The plan starts at the fires of the settle
     before it. The settle after the plan starts at the fires of both.
   - The plan-open check (plan_at_cap) and the cap check of a settle round
     (at_cap) are two definitions, as open_plan and settle make two checks
     in the code.
   - Section 15 counts one host-work dimension. Each dimension satisfies the
     rule alone.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. stack_head_is_max_ord              the top has the greatest
                                          (ord, rank)
    2. stack_produce_preserves_inv        a produce that completes in its
                                          round leaves INV
    3. stack_consume_preserves_inv        a consume that completes in its
                                          round leaves INV
    4. stack_lifo                         every fire binds the two tops
                                          (LifoOrder)
    5. stack_head_blocks                  a top that does not match blocks
                                          (for a cap above 0)
    6. stack_settle_terminates            a settle that starts after f
                                          fires of its transaction fires
                                          at most cap - f times and at most
                                          μ times (plus one divergent
                                          fire). Every transaction fires
                                          at most cap times, and a
                                          transaction that re-issues at the
                                          cap fired exactly cap times
                                          (I-21)
    7. stack_plan_permutation_invariant   the plans ignore the list order
    8. stack_without_settle_breaks_inv    without the settle INV fails
                                          (the computed counterexample)
    9. stack_reentry_at_lifo_place        a re-entry below a younger top is
                                          stored at its LIFO place (for a
                                          cap above 0)
   10. stack_reissue_keeps_ord            a re-issue keeps the first-round
                                          order, one bump per round
   11. stack_peek_restore_needs_no_settle a peek re-stores its record and
                                          starts no settle (for a cap
                                          above 0)
   12. settle_trace_covered               a settle reserves its reads
                                          before it makes them (DR-88)
   13. stack_settle_keeps_unspent_records a settle loses no record, from
                                          every fire count: only a fire of
                                          a waiter that does not peek
                                          spends a stored record
   The premise "a cap above 0" of headlines 5, 9 and 11 comes from
   open_plan: with a cap of 0, no plan opens (FIRE_ROUND_CAP is 64).

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
   sd-top-least-ord               above prefers the lesser order
                                  → above_false_ge
   sd-top-by-position             top is the first entry of the list
                                  → top_spec
   sd-no-settle-after             a completed plan has no settle after it
                                  → with_settles_inv
   sd-no-lift                     the settle does not lift a stored record
                                  → settle_step_records
   sd-land-drops                  the record in flight never lands
                                  → next_fire_none_land_inv
   sd-peek-spends-lifted          a fire of a peek waiter spends the lifted
                                  record → settle_loop_keeps
   sd-no-cap-check                at_cap is always false: a settle round
                                  has no cap check of the transaction
                                  → settle_loop_bounds
   sd-no-divergent-end            a divergent pair goes on
                                  → settle_loop_bounds
   sd-peek-no-restore             a peek of a record stages no re-store
                                  → stack_peek_restore_needs_no_settle
   sd-reentry-at-top              a re-entry plans whatever the top is
                                  → stack_lifo
   sd-reentry-empty-side-stores   a re-entry on an empty side is stored
                                  → stack_reentry_at_lifo_place
   sd-reissue-fresh-ord           carried is always None
                                  → stack_reissue_keeps_ord
   sd-no-bump                     the bump keeps the counter value
                                  → stack_reissue_keeps_ord
   sd-fire-blocked-top            the offer loop ignores the fit test
                                  → stack_head_blocks
   sd-perm-without-distinct-ords  dist holds for every view
                                  → apply_equiv
   sd-read-before-reserve         a step reads before it reserves
                                  → step_covered
   sd-unprepaid-unwrap            a round does not prepay the unwrap
                                  → round_staged_shape
   The controls of revision I-21 (one fire count for each transaction):
   sd-plan-opens-at-cap           plan_at_cap is always false: the plan
                                  opens after a settle that ended at the
                                  cap → stack_settle_terminates
   sd-plan-restarts-count         the plan starts at 0 fires, not at the
                                  fires of the settle before it
                                  → stack_settle_terminates
   sd-after-settle-restarts-count the settle after the plan starts at 0
                                  fires, so each settle call has its own
                                  count → stack_settle_terminates

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - One combined tactic chain over the four cases of a settle step (the
     record in flight or not, persistent or not) fails with "No product
     even after head-reduction". Prove the step facts as separate lemmas
     (settle_step_flight_ok, settle_step_inv, settle_step_measure).
   - destruct (w_pers w) abstracts only the goal. A hypothesis that holds
     "if w_pers w then 0 else 1" keeps it, and lia then fails. Revert the
     count hypotheses before the case split (settle_step_measure).
   - set (x := t) without "in *" leaves the expanded term in the
     hypotheses, and lia then sees two different atoms.
   - fold z does not fold settle_loop (S cap) 0 st None into
     z := settle_of st. Unfold z instead.
   - rewrite !total_app splits a balanced block into its atoms, so an
     equation over the whole block no longer matches. Prove a lemma over an
     abstract block (totals_prepaid).
   - In Section Tops the type variable is E, so a hypothesis named E by
     eqn:E clashes with it.
   - Rocq cannot infer an element that occurs only under a projection, such
     as r_ord ?r in a freshness premise. Pass the element explicitly.
   - with_settles_quiet takes the plan at the view itself, so a goal of it
     holds no run st [] to rewrite: use cbn beta.
   - A proof that destructs the literal cap test (cap <=? fired) or unfolds
     flight_after depends on the text of the rule. A control that changes
     the rule then breaks a lemma whose statement stays true. Name the rule
     (at_cap) and prove its facts once (flight_after_some).
   - A goal selector [a | b] after a chain that can close every goal fails
     when a control makes the statement vacuous (flight_after_some). Run
     the tail of the chain with all: instead.
   - The empty-side branch of a re-entry in stack_lifo applied
     offer_loop_lifo, so it assumed that the re-entry plans. A re-entry that
     stores is LIFO too. Accept both plans with first [...].
   The strategies below come from revision I-21 (one fire count for each
   transaction):
   - destruct (p_end p) eqn:E also rewrites the hypotheses that mention
     p_end p. So a bound P2 : p_end p = PReissue c -> ... becomes
     PReissue c = PReissue c -> ..., and specialize (P2 E) fails. Use
     try rewrite E in P2, and then specialize (P2 eq_refl).
   - Do not prove "plan_at_cap f = false -> f < cap" as a lemma. The control
     sd-plan-opens-at-cap makes that statement false, so the control then
     fails there and not in the bound headline. Derive f < cap inside
     stack_settle_terminates. plan_at_cap_false and plan_at_cap_true stay
     true under the control.
   - Some statements do not depend on the start count of the plan or of
     the settle after the plan. Do not write that count (f + fires (p_ops
     p)) in their proofs, or the count controls fail there. Find the plan
     and the settle after the plan with match goal or lazymatch instead.
     with_settles_inv, with_settles_lifo, with_settles_carried,
     with_settles_stores and with_settles_equiv do this.
   - Do not derive the transaction bound from settle_loop_bounds. It needs
     settle_inv (distinct orders) on the view after the plan.
     settle_loop_cap proves the cap bound with no premise on the view.
   - A lemma "after_fire n spent stays = FContinue -> n < cap" is false
     under sd-no-divergent-end. Put it after settle_loop_bounds, so that the
     control still fails first in settle_loop_bounds.
   ═══════════════════════════════════════════════════════════════════════ *)
From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── 1. The fire rounds ────────────────────────────────────────────────── *)

Inductive fire_step : Type := FDone | FContinue | FDivergent | FRoundCap.

Inductive reissue : Type := ReissueDivergent | ReissueRoundCap.

Section Stack.
  Variable V : Type.     (* the payload of a datum *)
  Variable P : Type.     (* the pattern and the guard of a continuation *)
  Variable fits : P -> V -> bool.
  Variable cap : nat.    (* FIRE_ROUND_CAP *)

  (* FireRounds::after_fire with the count of fires after this one. *)
  Definition after_fire (fired : nat) (spent stays : bool) : fire_step :=
    if spent then FDone
    else if stays then FDivergent
    else if cap <=? fired then FRoundCap
    else FContinue.

  (* ── 2. Entries and tops ─────────────────────────────────────────────── *)

  Record record : Type := mkRecord { r_ord : nat; r_rank : nat; r_val : V; r_pers : bool }.
  Record waiter : Type :=
    mkWaiter { w_ord : nat; w_rank : nat; w_pat : P; w_pers : bool; w_peek : bool }.

  Section Tops.
    Variable E : Type.
    Variable ord : E -> nat.
    Variable rank : E -> nat.

    (* above a b: a greater ord, or an equal ord and a greater rank. *)
    Definition above (a b : E) : bool :=
      (ord b <? ord a) || ((ord a =? ord b) && (rank b <? rank a)).

    (* key_ge a b: the key (ord, rank) of a is at least the key of b. *)
    Definition key_ge (a b : E) : Prop :=
      ord b < ord a \/ (ord a = ord b /\ rank b <= rank a).

    (* StackDiscipline::head_metered: a later entry replaces the top only
       when it is above it, so on equal keys the earlier entry stays. *)
    Fixpoint top_from (best : option (nat * E)) (idx : nat) (l : list E) : option (nat * E) :=
      match l with
      | [] => best
      | e :: rest =>
          let best' :=
            match best with
            | None => Some (idx, e)
            | Some (bi, b) => if above e b then Some (idx, e) else Some (bi, b)
            end in
          top_from best' (S idx) rest
      end.

    Definition top_index (l : list E) : option (nat * E) := top_from None 0 l.

    Definition top (l : list E) : option E := option_map snd (top_index l).

    Lemma above_false_ge : forall a b, above a b = false -> key_ge b a.
    Proof.
      intros a b H. unfold above in H.
      apply orb_false_iff in H as [H1 H2]. apply Nat.ltb_ge in H1.
      unfold key_ge.
      destruct (Nat.eq_dec (ord a) (ord b)) as [E1 | E1].
      - right. split; [symmetry; exact E1 |].
        rewrite (proj2 (Nat.eqb_eq _ _) E1) in H2. simpl in H2. apply Nat.ltb_ge in H2. exact H2.
      - left. lia.
    Qed.

    Lemma above_true_gt : forall a b, above a b = true -> key_ge a b /\ ~ key_ge b a.
    Proof.
      intros a b H. unfold above in H. unfold key_ge.
      apply orb_true_iff in H as [H | H].
      - apply Nat.ltb_lt in H. split; [left; exact H | lia].
      - apply andb_true_iff in H as [H1 H2]. apply Nat.eqb_eq in H1. apply Nat.ltb_lt in H2.
        split; [right; split; [exact H1 | lia] | lia].
    Qed.

    Lemma key_ge_trans : forall a b c, key_ge a b -> key_ge b c -> key_ge a c.
    Proof. unfold key_ge. intros a b c H1 H2. lia. Qed.

    Lemma key_ge_refl : forall a, key_ge a a.
    Proof. intros a. right. split; [reflexivity | lia]. Qed.

    Lemma ge_not_above : forall a b, key_ge b a -> above a b = false.
    Proof.
      intros a b H. destruct (above a b) eqn:Hab; [| reflexivity].
      apply above_true_gt in Hab as [_ N]. contradiction.
    Qed.

    (* The invariant of top_from: the best is an entry of the prefix with the
       greatest key, and its index is its position. *)
    Lemma top_from_spec :
      forall l pre best idx,
        idx = length pre ->
        (match best with
         | None => pre = []
         | Some (bi, b) => nth_error pre bi = Some b /\ forall x, In x pre -> key_ge b x
         end) ->
        match top_from best idx l with
        | None => pre ++ l = []
        | Some (bi, b) =>
            nth_error (pre ++ l) bi = Some b /\ forall x, In x (pre ++ l) -> key_ge b x
        end.
    Proof.
      induction l as [| e rest IH]; intros pre best idx Hidx Hbest.
      - simpl. rewrite app_nil_r. destruct best as [[bi b] |]; [exact Hbest | exact Hbest].
      - simpl. replace (pre ++ e :: rest) with ((pre ++ [e]) ++ rest)
          by (rewrite <- app_assoc; reflexivity).
        apply IH; [rewrite length_app; simpl; lia |].
        destruct best as [[bi b] |].
        + destruct Hbest as [Hnth Hmax]. destruct (above e b) eqn:Ea.
          * split.
            -- rewrite nth_error_app2 by lia. rewrite Hidx, Nat.sub_diag. reflexivity.
            -- intros x Hx. apply in_app_or in Hx as [Hx | [Hx | []]].
               ++ apply above_true_gt in Ea as [Hge _].
                  exact (key_ge_trans _ _ _ Hge (Hmax x Hx)).
               ++ subst. apply key_ge_refl.
          * split.
            -- rewrite nth_error_app1; [exact Hnth |].
               apply nth_error_Some. rewrite Hnth. discriminate.
            -- intros x Hx. apply in_app_or in Hx as [Hx | [Hx | []]].
               ++ exact (Hmax x Hx).
               ++ subst. apply above_false_ge. exact Ea.
        + subst pre. simpl in Hidx. subst idx. split; [reflexivity |].
          intros x [Hx | []]. subst. apply key_ge_refl.
    Qed.

    Lemma top_index_spec :
      forall l,
        match top_index l with
        | None => l = []
        | Some (bi, b) => nth_error l bi = Some b /\ forall x, In x l -> key_ge b x
        end.
    Proof. intros l. exact (top_from_spec l [] None 0 eq_refl eq_refl). Qed.

    Lemma top_spec : forall l t, top l = Some t -> In t l /\ forall x, In x l -> key_ge t x.
    Proof.
      intros l t H. unfold top in H. pose proof (top_index_spec l) as Hs.
      destruct (top_index l) as [[bi b] |]; [| discriminate].
      injection H as <-. destruct Hs as [Hnth Hmax].
      split; [apply nth_error_In with bi; exact Hnth | exact Hmax].
    Qed.

    Lemma top_none : forall l, top l = None <-> l = [].
    Proof.
      intros l. unfold top. pose proof (top_index_spec l) as Hs.
      destruct (top_index l) as [[bi b] |]; simpl; split; intro H.
      - discriminate.
      - subst l. destruct Hs as [Hnth _]. destruct bi; discriminate.
      - exact Hs.
      - reflexivity.
    Qed.

    Lemma top_some : forall l, l <> [] -> exists t, top l = Some t.
    Proof.
      intros l Hl. destruct (top l) as [t |] eqn:Htop; [exists t; reflexivity |].
      apply top_none in Htop. contradiction.
    Qed.

    (* The ords of a list are distinct. *)
    Definition ords_distinct (l : list E) : Prop := NoDup (map ord l).

    Lemma ords_distinct_inj :
      forall l a b, ords_distinct l -> In a l -> In b l -> ord a = ord b -> a = b.
    Proof.
      induction l as [| e rest IH]; intros a b Hnd Ha Hb Hab; [destruct Ha |].
      unfold ords_distinct in Hnd. simpl in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
      destruct Ha as [Ha | Ha]; destruct Hb as [Hb | Hb]; subst.
      - reflexivity.
      - exfalso. apply Hnot. rewrite Hab. apply in_map. exact Hb.
      - exfalso. apply Hnot. rewrite <- Hab. apply in_map. exact Ha.
      - exact (IH a b Hnd Ha Hb Hab).
    Qed.

    (* With distinct ords, an entry whose key is at least every key is the top. *)
    Lemma top_is_max :
      forall l t, ords_distinct l -> In t l -> (forall x, In x l -> key_ge t x) -> top l = Some t.
    Proof.
      intros l t Hnd Ht Hmax.
      destruct (top_some l) as [t' Ht']; [intros Hnil; subst; destruct Ht |].
      rewrite Ht'. destruct (top_spec l t' Ht') as [Hin' Hmax'].
      pose proof (Hmax t' Hin') as G1. pose proof (Hmax' t Ht) as G2.
      unfold key_ge in G1, G2. f_equal.
      apply (ords_distinct_inj l t' t Hnd Hin' Ht). lia.
    Qed.

    (* An entry whose key is at least every key of l stays on top when it is
       put in front of l, as the hot store puts a re-stored record. *)
    Lemma top_front_max :
      forall e l, (forall x, In x l -> key_ge e x) -> top (e :: l) = Some e.
    Proof.
      intros e l Hmax. unfold top, top_index. simpl.
      assert (G : forall l' idx, (forall x, In x l' -> key_ge e x) ->
                    top_from (Some (0, e)) idx l' = Some (0, e)).
      { induction l' as [| y rest IH]; intros idx Hm; [reflexivity |].
        simpl. rewrite ge_not_above by (apply Hm; left; reflexivity).
        apply IH. intros x Hx. apply Hm. right. exact Hx. }
      rewrite (G l 1 Hmax). reflexivity.
    Qed.
  End Tops.

  Arguments above {E} ord rank a b.
  Arguments key_ge {E} ord rank a b.
  Arguments top_index {E} ord rank l.
  Arguments top {E} ord rank l.
  Arguments ords_distinct {E} ord l.

  Definition record_top (l : list record) : option record := top r_ord r_rank l.
  Definition waiter_top (l : list waiter) : option waiter := top w_ord w_rank l.

  (* Headline 1. The top of each side is an entry of that side with the
     greatest (ord, rank), and a side has a top exactly when it is not
     empty. *)
  Theorem stack_head_is_max_ord :
    (forall l r, record_top l = Some r ->
       In r l /\ forall x, In x l -> key_ge r_ord r_rank r x) /\
    (forall l w, waiter_top l = Some w ->
       In w l /\ forall x, In x l -> key_ge w_ord w_rank w x) /\
    (forall l, record_top l = None <-> l = []) /\
    (forall l, waiter_top l = None <-> l = []).
  Proof.
    split; [| split; [| split]].
    - intros l r H. exact (top_spec record r_ord r_rank l r H).
    - intros l w H. exact (top_spec waiter w_ord w_rank l w H).
    - intros l. exact (top_none record r_ord r_rank l).
    - intros l. exact (top_none waiter w_ord w_rank l).
  Qed.

  (* ── 3. States, staged operations and their effects ───────────────────── *)

  (* A collection: its records, its waiting continuations, and its counter
     (None before the first operation). *)
  Record sstate : Type :=
    mkS { s_records : list record; s_waiters : list waiter; s_counter : option nat }.

  (* The bump of spaces/counter.rs: the ord is the counter value, or 0 for
     the first operation, which opens the counter at 1. *)
  Definition bump_ord (c : option nat) : nat := match c with None => 0 | Some v => v end.
  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).
  Definition bumped (st : sstate) : sstate :=
    mkS (s_records st) (s_waiters st) (bump_next (s_counter st)).

  (* A staged operation of a transaction. *)
  Inductive staged : Type :=
  | SStore : record -> staged                 (* the store of the datum of the operation *)
  | SPark : waiter -> staged                  (* the park of its continuation *)
  | SFireProduce : waiter -> record -> staged (* a produce-side fire: the record is incoming *)
  | SFireConsume : waiter -> record -> staged (* a consume-side fire: the waiter is incoming *)
  | SLift : record -> staged                  (* K_lift removes a stored record *)
  | SRestore : record -> staged.              (* Rule E: the record is stored again *)

  (* The removal of the first entry with the key (o, k): the substrate
     removes a record or a waiter by its identity. *)
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
     record in front (hot_store.rs: put_datum inserts at index 0). A fire
     removes the fired waiter unless it is persistent. A produce-side fire
     never stores its incoming record. A consume-side fire removes the
     record that it binds unless it is persistent, also for a peek (Rule E),
     and the code then stores the record again (SRestore). *)
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

  (* The staged view after a list of staged operations. *)
  Definition run (st : sstate) (ops : list staged) : sstate := fold_left apply ops st.

  Lemma run_app : forall st a b, run st (a ++ b) = run (run st a) b.
  Proof. intros st a b. unfold run. apply fold_left_app. Qed.

  Lemma run_cons : forall st op rest, run st (op :: rest) = run (apply st op) rest.
  Proof. reflexivity. Qed.

  Lemma run_nil : forall st, run st [] = st.
  Proof. reflexivity. Qed.

  (* The fires of a list of staged operations: the fires that the outcome
     dispatches. *)
  Definition is_fire (op : staged) : bool :=
    match op with SFireProduce _ _ | SFireConsume _ _ => true | _ => false end.
  Definition fires (ops : list staged) : nat := length (filter is_fire ops).

  Lemma fires_app : forall a b, fires (a ++ b) = fires a + fires b.
  Proof. intros a b. unfold fires. rewrite filter_app, length_app. reflexivity. Qed.

  (* ── 4. The settle (settle.rs) ──────────────────────────────────────── *)

  Inductive settle_end : Type := Quiescent | SettleReissue (c : reissue).

  Record settled : Type := mkSettled { z_ops : list staged; z_end : settle_end }.

  (* The landing of the record in flight: it is stored again at its order,
     with its own bytes, so with its identity. *)
  Definition land (x : option record) : list staged :=
    match x with None => [] | Some r => [SRestore r] end.

  (* StackView::next_fire: the top waiter and the top record, or the record
     in flight, when the top waiter fits the record (the spatial match and
     then the guard, decision 37). *)
  Definition next_fire (st : sstate) (x : option record) : option (waiter * record) :=
    match waiter_top (s_waiters st) with
    | None => None
    | Some w =>
        match (match x with Some r => Some r | None => record_top (s_records st) end) with
        | None => None
        | Some r => if fits (w_pat w) (r_val r) then Some (w, r) else None
        end
    end.

  (* The lift of a fire of the settle: a stored record that is not
     persistent leaves the store before its re-offer. A record in flight is
     already out of the store. *)
  Definition lift_ops (x : option record) (r : record) : list staged :=
    match x with Some _ => [] | None => if r_pers r then [] else [SLift r] end.

  (* True when the record of the fire is out of the store: lifted now, or
     already in flight. *)
  Definition flying (x : option record) (r : record) : bool :=
    match x with Some _ => true | None => negb (r_pers r) end.

  (* The record in flight after a fire: the lifted record when the fire did
     not spend it. *)
  Definition flight_after (x : option record) (r : record) (spent : bool) : option record :=
    if flying x r && negb spent then Some r else None.

  (* The cap check at the top of a settle round: the transaction has fired
     cap times (I-21). fired counts the fires of the whole transaction. *)
  Definition at_cap (fired : nat) : bool := cap <=? fired.

  (* settle: the loop of settle.rs. A stored record that is not persistent is
     lifted, and the record is offered again to the waiter. A record that a
     waiter that peeks did not spend stays in flight (decision 30). At the
     cap or at a divergent pair the record in flight lands and the settle
     re-issues. *)
  Fixpoint settle_loop (fuel fired : nat) (st : sstate) (x : option record) : settled :=
    match fuel with
    | 0 => mkSettled (land x) (SettleReissue ReissueRoundCap)
    | S fuel' =>
        match next_fire st x with
        | None => mkSettled (land x) Quiescent
        | Some (w, r) =>
            if at_cap fired then mkSettled (land x) (SettleReissue ReissueRoundCap)
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

  (* A settle call that starts its transaction: the settle before the plan
     and the settle of a settle job. It starts at 0 fires, and it makes at
     most cap fires, so cap + 1 rounds of the loop are enough. *)
  Definition settle_of (st : sstate) : settled := settle_loop (S cap) 0 st None.

  (* The settle after the plan (I-21): it continues the fire count of its
     transaction from fired. At most cap - fired fires remain, so cap + 1
     rounds of the loop are enough. *)
  Definition settle_at (fired : nat) (st : sstate) : settled := settle_loop (S cap) fired st None.

  (* ── 5. The plans of an operation (offer_head and receive_head) ─────── *)

  Inductive plan_end : Type := PDone | PReissue (c : reissue).

  Record planned : Type := mkPlanned { p_ops : list staged; p_end : plan_end }.

  (* The loop of offer_head for the datum r on top of its side. *)
  Fixpoint offer_loop (fuel fired : nat) (st : sstate) (r : record) : planned :=
    match fuel with
    | 0 => mkPlanned [] (PReissue ReissueRoundCap)
    | S fuel' =>
        match waiter_top (s_waiters st) with
        | None => mkPlanned [SStore r] PDone
        | Some w =>
            if fits (w_pat w) (r_val r) then
              let spent := negb (r_pers r) && negb (w_peek w) in
              match after_fire (S fired) spent (w_pers w) with
              | FDone => mkPlanned [SFireProduce w r] PDone
              | FDivergent => mkPlanned [SFireProduce w r] (PReissue ReissueDivergent)
              | FRoundCap => mkPlanned [SFireProduce w r] (PReissue ReissueRoundCap)
              | FContinue =>
                  let rest := offer_loop fuel' (S fired) (apply st (SFireProduce w r)) r in
                  mkPlanned (SFireProduce w r :: p_ops rest) (p_end rest)
              end
            else mkPlanned [SStore r] PDone
        end
    end.

  (* A consume-side fire, and the re-store of decision 45 (Rule E) after a
     peek of a record that is not persistent. *)
  Definition consume_fire (k : waiter) (h : record) : list staged :=
    if w_peek k && negb (r_pers h) then [SFireConsume k h; SRestore h] else [SFireConsume k h].

  (* The loop of receive_head for the continuation k on top of its side. *)
  Fixpoint receive_loop (fuel fired : nat) (st : sstate) (k : waiter) : planned :=
    match fuel with
    | 0 => mkPlanned [] (PReissue ReissueRoundCap)
    | S fuel' =>
        match record_top (s_records st) with
        | None => mkPlanned [SPark k] PDone
        | Some h =>
            if fits (w_pat k) (r_val h) then
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

  (* ── 6. Operations, re-entries and settle jobs ───────────────────────── *)

  (* The next round of an operation: the operation itself, with the
     first-round order of a persistent element (decision 43), or a settle
     job. *)
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

  (* The plan-open check of open_plan (I-21): the settle before the plan
     left the transaction at the cap. The plan then stages nothing, and the
     operation re-issues with the order rule of decision 43. *)
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

  (* offer_head and receive_head around their loops: open_plan (the settle
     before the plan and the plan-open check), the plan, and the settle
     after the plan. One fire count runs through the transaction (I-21,
     Cx::rounds). The settle before the plan starts it at 0. The plan
     starts at the fires of that settle. The settle after the plan starts
     at the fires of both. A settle before the plan that cannot complete
     re-issues the operation. A settle before the plan that ends at the cap
     leaves no fire for the plan, so the operation re-issues with no plan.
     A plan that re-issues ends the round with no settle after it. *)
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

  (* run_with of generic.rs: the bump, then the plans with the ord of the
     bump. A fresh element always reaches the top (reaches_head is true).
     The plan starts at the fire count f that the settle before it left. *)
  Definition produce (st : sstate) (rank : nat) (v : V) (pers : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    with_settles (bumped st) pers o (fun f s => offer_loop (S cap) f s (mkRecord o rank v pers)).

  Definition consume (st : sstate) (rank : nat) (pat : P) (pers peek : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    with_settles (bumped st) pers o
      (fun f s => receive_loop (S cap) f s (mkWaiter o rank pat pers peek)).

  (* A settle job (SpaceOp::Settle): the bump, then one settle. A settle job
     is a transaction of its own, so its settle starts at 0 fires. *)
  Definition settle_job (st : sstate) : outcome :=
    let z := settle_of (bumped st) in
    mkOutcome (z_ops z) (settle_next (z_end z)).

  (* The re-entry of a re-issued persistent element x with its first-round
     order (decision 43): at the top when its side is empty or its order is
     greater than the order of the top (StackDiscipline::precedes), and
     otherwise stored or parked at its LIFO place, with no fire. The plan
     starts at the fire count f of its transaction. *)
  Definition reenter_offer (x : record) (f : nat) (s : sstate) : planned :=
    match record_top (s_records s) with
    | Some h => if r_ord h <? r_ord x then offer_loop (S cap) f s x else mkPlanned [SStore x] PDone
    | None => offer_loop (S cap) f s x
    end.

  Definition reenter_receive (k : waiter) (f : nat) (s : sstate) : planned :=
    match waiter_top (s_waiters s) with
    | Some w => if w_ord w <? w_ord k then receive_loop (S cap) f s k else mkPlanned [SPark k] PDone
    | None => receive_loop (S cap) f s k
    end.

  Definition reenter_produce (st : sstate) (x : record) : outcome :=
    with_settles (bumped st) (r_pers x) (r_ord x) (reenter_offer x).

  Definition reenter_consume (st : sstate) (k : waiter) : outcome :=
    with_settles (bumped st) (w_pers k) (w_ord k) (reenter_receive k).

  (* The staged view after an operation on st. *)
  Definition after (st : sstate) (o : outcome) : sstate := run (bumped st) (o_ops o).

  (* ── 7. INV, and the settle that restores it ─────────────────────────── *)

  (* INV: the top record and the top waiter do not match. *)
  Definition no_pending_match (st : sstate) : Prop :=
    match record_top (s_records st), waiter_top (s_waiters st) with
    | Some r, Some w => fits (w_pat w) (r_val r) = false
    | _, _ => True
    end.

  (* A record in flight has a key at least the key of every stored record:
     it was the top record when the settle lifted it. *)
  Definition flight_ok (st : sstate) (x : option record) : Prop :=
    match x with
    | None => True
    | Some r => forall h, In h (s_records st) -> key_ge r_ord r_rank r h
    end.

  Lemma drop_key_incl :
    forall (E : Type) (ord rank : E -> nat) o k l x, In x (drop_key ord rank o k l) -> In x l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; intros x Hx; [destruct Hx |].
    simpl in Hx. destruct ((ord y =? o) && (rank y =? k)).
    - right. exact Hx.
    - destruct Hx as [Hx | Hx]; [left; exact Hx | right; apply IH; exact Hx].
  Qed.

  Lemma next_fire_some :
    forall st x w r,
      next_fire st x = Some (w, r) ->
      waiter_top (s_waiters st) = Some w /\ fits (w_pat w) (r_val r) = true /\
      match x with Some r0 => r = r0 | None => record_top (s_records st) = Some r end.
  Proof.
    intros st x w r H. unfold next_fire in H.
    destruct (waiter_top (s_waiters st)) as [w0 |]; [| discriminate].
    destruct x as [r0 |].
    - destruct (fits (w_pat w0) (r_val r0)) eqn:Ef; [| discriminate].
      injection H as <- <-. split; [reflexivity | split; [exact Ef | reflexivity]].
    - destruct (record_top (s_records st)) as [h |]; [| discriminate].
      destruct (fits (w_pat w0) (r_val h)) eqn:Ef; [| discriminate].
      injection H as <- <-. split; [reflexivity | split; [exact Ef | reflexivity]].
  Qed.

  (* No fire with nothing in flight is INV. *)
  Lemma next_fire_none_inv : forall st, next_fire st None = None -> no_pending_match st.
  Proof.
    intros st H. unfold next_fire in H. unfold no_pending_match.
    destruct (waiter_top (s_waiters st)) as [w |];
      destruct (record_top (s_records st)) as [h |]; try exact I.
    destruct (fits (w_pat w) (r_val h)) eqn:Ef; [discriminate | reflexivity].
  Qed.

  (* No fire for a record in flight, and its landing: INV. *)
  Lemma next_fire_none_land_inv :
    forall st r, flight_ok st (Some r) -> next_fire st (Some r) = None ->
      no_pending_match (run st (land (Some r))).
  Proof.
    intros st r Hok H. unfold next_fire in H. cbn [land run fold_left apply].
    unfold no_pending_match. cbn [s_records s_waiters].
    unfold record_top. rewrite (top_front_max record r_ord r_rank r (s_records st) Hok).
    destruct (waiter_top (s_waiters st)) as [w |]; [| exact I].
    destruct (fits (w_pat w) (r_val r)) eqn:Ef; [discriminate | reflexivity].
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

  (* After a fire of the settle, the record in flight has a key at least the
     key of every stored record: it was the record in flight, or it was the
     top record before its lift. *)
  Lemma settle_step_flight_ok :
    forall st x w r spent,
      flight_ok st x -> next_fire st x = Some (w, r) ->
      flight_ok (run st (lift_ops x r ++ [SFireProduce w r])) (flight_after x r spent).
  Proof.
    intros st x w r spent Hok Hnf.
    pose proof (next_fire_some st x w r Hnf) as [_ [_ Hr]].
    destruct (flight_after x r spent) as [r1 |] eqn:Ef; [| exact I].
    destruct (flight_after_some x r spent r1 Ef) as [Hr1 [Hfly _]]. subst r1.
    unfold flight_ok. intros h Hh. rewrite settle_step_records in Hh.
    destruct x as [r0 |].
    - subst r0. exact (Hok h Hh).
    - unfold flying in Hfly. destruct (r_pers r) eqn:Ep; [discriminate |].
      apply (proj2 (top_spec record r_ord r_rank _ r Hr)).
      unfold drop_record in Hh. eapply drop_key_incl. exact Hh.
  Qed.

  (* A settle that ends Quiescent leaves INV. *)
  Lemma settle_quiescent_inv :
    forall fuel fired st x,
      flight_ok st x ->
      z_end (settle_loop fuel fired st x) = Quiescent ->
      no_pending_match (run st (z_ops (settle_loop fuel fired st x))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x Hok Hend; cbn [settle_loop] in *.
    - discriminate.
    - destruct (next_fire st x) as [[w r] |] eqn:Hnf.
      + destruct (at_cap fired); [discriminate |].
        destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
          cbn [z_end z_ops] in *; try discriminate;
          rewrite run_app; apply IH; try exact Hend;
          apply settle_step_flight_ok; assumption.
      + destruct x as [r |].
        * apply next_fire_none_land_inv; [exact Hok | exact Hnf].
        * cbn [land z_ops]. rewrite run_nil. apply next_fire_none_inv. exact Hnf.
  Qed.

  (* An operation that completes in its round leaves INV. The settle after
     the plan ends Quiescent from every fire count. *)
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
    apply settle_quiescent_inv; [exact I | exact H].
  Qed.

  (* Headline 2. A produce that completes in its round leaves INV: the top
     record and the top waiter do not match. The settle after the plan
     restores INV, so the state before the produce needs no premise. *)
  Theorem stack_produce_preserves_inv :
    forall st rank v pers,
      o_next (produce st rank v pers) = None -> no_pending_match (after st (produce st rank v pers)).
  Proof. intros st rank v pers H. unfold after, produce in *. apply with_settles_inv. exact H. Qed.

  (* Headline 3. A consume that completes in its round leaves INV. *)
  Theorem stack_consume_preserves_inv :
    forall st rank pat pers peek,
      o_next (consume st rank pat pers peek) = None ->
      no_pending_match (after st (consume st rank pat pers peek)).
  Proof.
    intros st rank pat pers peek H. unfold after, consume in *. apply with_settles_inv. exact H.
  Qed.

  (* ── 8. LIFO: every fire binds the two tops ─────────────────────────── *)

  Definition rec_ge (a b : record) : Prop := key_ge r_ord r_rank a b.
  Definition wtr_ge (a b : waiter) : Prop := key_ge w_ord w_rank a b.

  (* A fire binds the top of each side of the view where it is staged. The
     incoming side of a fire (the datum or the record in flight of a
     produce-side fire, the continuation of a consume-side fire) has a key at
     least the key of every stored entry of its side, so it is the top of its
     side. A lift takes the top record. *)
  Definition at_tops (st : sstate) (op : staged) : Prop :=
    match op with
    | SFireProduce w r =>
        waiter_top (s_waiters st) = Some w /\ forall h, In h (s_records st) -> rec_ge r h
    | SFireConsume k h =>
        record_top (s_records st) = Some h /\ forall w, In w (s_waiters st) -> wtr_ge k w
    | SLift h => record_top (s_records st) = Some h
    | SStore _ | SPark _ | SRestore _ => True
    end.

  (* LifoOrder over a list of staged operations from the view st. *)
  Fixpoint lifo_trace (st : sstate) (ops : list staged) : Prop :=
    match ops with
    | [] => True
    | op :: rest => at_tops st op /\ lifo_trace (apply st op) rest
    end.

  Lemma lifo_trace_app :
    forall a b st, lifo_trace st a -> lifo_trace (run st a) b -> lifo_trace st (a ++ b).
  Proof.
    induction a as [| op rest IH]; intros b st Ha Hb; [exact Hb |].
    destruct Ha as [Hop Hrest]. split; [exact Hop |]. apply IH; [exact Hrest | exact Hb].
  Qed.

  Lemma land_lifo : forall st x, lifo_trace st (land x).
  Proof. intros st [r |]; simpl; [split; exact I | exact I]. Qed.

  (* Every fire of a settle binds the two tops. *)
  Lemma settle_lifo :
    forall fuel fired st x, flight_ok st x -> lifo_trace st (z_ops (settle_loop fuel fired st x)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st x Hok; cbn [settle_loop].
    - apply land_lifo.
    - destruct (next_fire st x) as [[w r] |] eqn:Hnf; [| apply land_lifo].
      destruct (at_cap fired); [apply land_lifo |].
      pose proof (next_fire_some st x w r Hnf) as [Hw [_ Hr]].
      assert (Hstep : lifo_trace st (lift_ops x r ++ [SFireProduce w r])).
      { destruct x as [r0 |].
        - subst r0. cbn [lift_ops app lifo_trace at_tops].
          split; [split; [exact Hw | exact Hok] | exact I].
        - destruct (top_spec record r_ord r_rank _ r Hr) as [_ Hmax].
          unfold lift_ops. destruct (r_pers r).
          + cbn [app lifo_trace at_tops].
            split; [split; [exact Hw | exact Hmax] | exact I].
          + cbn [app lifo_trace at_tops apply s_records s_waiters].
            split; [exact Hr |]. split; [| exact I]. split; [exact Hw |].
            intros h Hh. apply Hmax. unfold drop_record in Hh. eapply drop_key_incl. exact Hh. }
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        cbn [z_ops]; apply lifo_trace_app; try exact Hstep;
        solve [apply land_lifo | apply IH; apply settle_step_flight_ok; assumption].
  Qed.

  Lemma fire_produce_records :
    forall st w r, s_records (apply st (SFireProduce w r)) = s_records st.
  Proof. intros st w r. cbn [apply]. destruct (w_pers w); reflexivity. Qed.

  (* Every fire of the produce plan binds the top waiter, and the datum is on
     top of the records. *)
  Lemma offer_loop_lifo :
    forall fuel fired st r,
      (forall h, In h (s_records st) -> rec_ge r h) -> lifo_trace st (p_ops (offer_loop fuel fired st r)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hr; cbn [offer_loop]; [exact I |].
    destruct (waiter_top (s_waiters st)) as [w |] eqn:Hw; [| split; exact I].
    destruct (fits (w_pat w) (r_val r)); [| try (split; exact I)].
    all: destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [p_ops lifo_trace at_tops];
      try (split; [split; [exact Hw | exact Hr] | exact I]).
    all: split; [split; [exact Hw | exact Hr] |];
      apply IH; rewrite fire_produce_records; exact Hr.
  Qed.

  Lemma consume_fire_waiters :
    forall st k h, s_waiters (run st (consume_fire k h)) = s_waiters st.
  Proof.
    intros st k h. unfold consume_fire.
    destruct (w_peek k && negb (r_pers h)); cbn [run fold_left apply];
      destruct (r_pers h); reflexivity.
  Qed.

  Lemma consume_fire_lifo :
    forall st k h,
      record_top (s_records st) = Some h -> (forall w, In w (s_waiters st) -> wtr_ge k w) ->
      lifo_trace st (consume_fire k h).
  Proof.
    intros st k h Hh Hk. unfold consume_fire.
    destruct (w_peek k && negb (r_pers h)); cbn [lifo_trace at_tops];
      split; try (split; [exact Hh | exact Hk]); repeat split.
  Qed.

  (* Every fire of the consume plan binds the top record, and the
     continuation is on top of the waiters. *)
  Lemma receive_loop_lifo :
    forall fuel fired st k,
      (forall w, In w (s_waiters st) -> wtr_ge k w) ->
      lifo_trace st (p_ops (receive_loop fuel fired st k)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hk; cbn [receive_loop]; [exact I |].
    destruct (record_top (s_records st)) as [h |] eqn:Hh; [| split; exact I].
    destruct (fits (w_pat k) (r_val h)); [| split; exact I].
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
      cbn [p_ops]; try (apply consume_fire_lifo; assumption).
    apply lifo_trace_app; [apply consume_fire_lifo; assumption |].
    apply IH. rewrite consume_fire_waiters. exact Hk.
  Qed.

  (* Every fire of an operation binds the two tops, when its plan does from
     every fire count. *)
  Lemma with_settles_lifo :
    forall st pers o plan,
      (forall f, lifo_trace (run st (z_ops (settle_of st)))
                            (p_ops (plan f (run st (z_ops (settle_of st)))))) ->
      lifo_trace st (o_ops (with_settles st pers o plan)).
  Proof.
    intros st pers o plan Hplan. unfold with_settles.
    pose proof (settle_lifo (S cap) 0 st None I) as Hbefore.
    destruct (z_end (settle_of st)); cbn [o_ops]; [| exact Hbefore].
    destruct (plan_at_cap (fires (z_ops (settle_of st)))); cbn [o_ops]; [exact Hbefore |].
    match goal with |- context [p_end ?p] => destruct (p_end p) end; cbn [o_ops].
    - apply lifo_trace_app; [exact Hbefore |].
      apply lifo_trace_app; [exact (Hplan _) |].
      rewrite <- run_app. apply settle_lifo. exact I.
    - apply lifo_trace_app; [exact Hbefore | exact (Hplan _)].
  Qed.

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
    - destruct (next_fire st x) as [[w r] |] eqn:Hnf; [| apply Hland; assumption].
      destruct (at_cap fired); [apply Hland; assumption |].
      pose proof (next_fire_some st x w r Hnf) as [_ [_ Hr]].
      assert (Hrn : r_ord r < n).
      { destruct x as [r0 |]; [subst; exact Hx |].
        apply (proj1 Hst). exact (proj1 (top_spec record r_ord r_rank _ r Hr)). }
      assert (Hstep : ords_below n (run st (lift_ops x r ++ [SFireProduce w r]))).
      { destruct Hst as [Hrec Hwt]. split.
        - rewrite settle_step_records. destruct x; [exact Hrec |].
          destruct (r_pers r); [exact Hrec | apply drop_record_below; exact Hrec].
        - unfold lift_ops. destruct x as [r0 |]; [| destruct (r_pers r)];
            cbn [app run fold_left apply s_waiters]; destruct (w_pers w); cbn [s_waiters];
            first [exact Hwt | apply drop_waiter_below; exact Hwt]. }
      assert (Hx1 : forall spent, match flight_after x r spent with
                                   | Some r1 => r_ord r1 < n | None => True end).
      { intros spent. destruct (flight_after x r spent) as [r1 |] eqn:Ef; [| exact I].
        destruct (flight_after_some x r spent r1 Ef) as [Hr1 _]. subst r1. exact Hrn. }
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        cbn [z_ops]; rewrite run_app;
        solve [apply IH; [exact Hstep | apply Hx1] | apply Hland; [exact Hstep | apply Hx1]].
  Qed.

  Lemma below_rec_ge :
    forall n st r, ords_below n st -> r_ord r = n -> forall h, In h (s_records st) -> rec_ge r h.
  Proof. intros n st r [Hr _] Hn h Hh. left. rewrite Hn. exact (Hr h Hh). Qed.

  Lemma below_wtr_ge :
    forall n st k, ords_below n st -> w_ord k = n -> forall w, In w (s_waiters st) -> wtr_ge k w.
  Proof. intros n st k [_ Hw] Hn w Hin. left. rewrite Hn. exact (Hw w Hin). Qed.

  (* Headline 4. LIFO (LifoOrder): every fire of an operation, of a settle
     job and of a re-entry binds the top waiter and the top record. The
     incoming side of each fire is above every stored entry of its side. So
     records and waiters leave their sides from the top, in LIFO order. The
     premise for a fresh operation is that the counter exceeds every order
     (wf). *)
  Theorem stack_lifo :
    (forall st rank v pers, ords_below (bump_ord (s_counter st)) st ->
       lifo_trace (bumped st) (o_ops (produce st rank v pers))) /\
    (forall st rank pat pers peek, ords_below (bump_ord (s_counter st)) st ->
       lifo_trace (bumped st) (o_ops (consume st rank pat pers peek))) /\
    (forall st, lifo_trace (bumped st) (o_ops (settle_job st))) /\
    (forall st x, lifo_trace (bumped st) (o_ops (reenter_produce st x))) /\
    (forall st k, lifo_trace (bumped st) (o_ops (reenter_consume st k))).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros st rank v pers Hb. unfold produce. apply with_settles_lifo. intros f.
      apply offer_loop_lifo. eapply below_rec_ge; [| reflexivity].
      apply settle_ords_below; [exact Hb | exact I].
    - intros st rank pat pers peek Hb. unfold consume. apply with_settles_lifo. intros f.
      apply receive_loop_lifo. eapply below_wtr_ge; [| reflexivity].
      apply settle_ords_below; [exact Hb | exact I].
    - intros st. unfold settle_job. cbn [o_ops]. apply settle_lifo. exact I.
    - intros st x. unfold reenter_produce. apply with_settles_lifo. intros f. unfold reenter_offer.
      set (s := run (bumped st) (z_ops (settle_of (bumped st)))).
      destruct (record_top (s_records s)) as [h |] eqn:Hh.
      + destruct (r_ord h <? r_ord x) eqn:Hlt; [| split; exact I].
        apply offer_loop_lifo. intros h' Hh'. apply Nat.ltb_lt in Hlt.
        pose proof (proj2 (top_spec record r_ord r_rank _ h Hh) h' Hh') as G.
        unfold rec_ge, key_ge in *. left. lia.
      + first [split; exact I | apply offer_loop_lifo; intros h' Hh';
          apply (top_none record r_ord r_rank) in Hh; rewrite Hh in Hh'; destruct Hh'].
    - intros st k. unfold reenter_consume. apply with_settles_lifo. intros f. unfold reenter_receive.
      set (s := run (bumped st) (z_ops (settle_of (bumped st)))).
      destruct (waiter_top (s_waiters s)) as [w |] eqn:Hw.
      + destruct (w_ord w <? w_ord k) eqn:Hlt; [| split; exact I].
        apply receive_loop_lifo. intros w' Hw'. apply Nat.ltb_lt in Hlt.
        pose proof (proj2 (top_spec waiter w_ord w_rank _ w Hw) w' Hw') as G.
        unfold wtr_ge, key_ge in *. left. lia.
      + first [split; exact I | apply receive_loop_lifo; intros w' Hw';
          apply (top_none waiter w_ord w_rank) in Hw; rewrite Hw in Hw'; destruct Hw'].
  Qed.

  (* ── 9. Quiet settles: head blocking, re-entry, peeks ────────────────── *)

  (* A settle of a view with INV stages nothing and ends Quiescent, from
     every fire count. *)
  Lemma settle_at_quiet :
    forall fired st, no_pending_match st -> settle_at fired st = mkSettled [] Quiescent.
  Proof.
    intros fired st H. unfold settle_at. cbn [settle_loop].
    assert (Hnf : next_fire st None = None).
    { unfold next_fire. unfold no_pending_match in H.
      destruct (waiter_top (s_waiters st)) as [w |]; [| reflexivity].
      destruct (record_top (s_records st)) as [h |]; [| reflexivity].
      rewrite H. reflexivity. }
    rewrite Hnf. reflexivity.
  Qed.

  Lemma settle_quiet : forall st, no_pending_match st -> settle_of st = mkSettled [] Quiescent.
  Proof. intros st H. exact (settle_at_quiet 0 st H). Qed.

  Lemma bumped_inv : forall st, no_pending_match st -> no_pending_match (bumped st).
  Proof. intros st H. exact H. Qed.

  (* The view after one store of the operation and the bump. *)
  Lemma store_after_bump :
    forall st r, run (bumped st) [SStore r] = mkS (r :: s_records st) (s_waiters st) (bump_next (s_counter st)).
  Proof. reflexivity. Qed.

  Lemma park_after_bump :
    forall st k, run (bumped st) [SPark k] = mkS (s_records st) (k :: s_waiters st) (bump_next (s_counter st)).
  Proof. reflexivity. Qed.

  (* The outcome of with_settles when both settles are quiet and the plan
     completes. The quiet settle before the plan fires nothing, so the plan
     opens at 0 fires when the cap is above 0. *)
  Lemma with_settles_quiet :
    forall st pers o plan ops,
      0 < cap ->
      no_pending_match st ->
      plan 0 st = mkPlanned ops PDone ->
      no_pending_match (run st ops) ->
      with_settles st pers o plan = mkOutcome ops None.
  Proof.
    intros st pers o plan ops Hcap Hinv Hplan Hafter. unfold with_settles.
    rewrite (settle_quiet st Hinv). cbn [z_end z_ops].
    change (fires []) with 0. rewrite (plan_at_cap_false 0 Hcap), run_nil, Hplan.
    cbn [p_end p_ops app]. rewrite (settle_at_quiet _ (run st ops) Hafter).
    cbn [z_ops z_end settle_next]. rewrite app_nil_r. reflexivity.
  Qed.

  (* Headline 5. A top that does not match blocks. When the datum does not
     fit the top waiter, the produce stores the datum on top and fires
     nothing, although a waiter below the top may fit it. When the top
     record does not fit the pattern of the receive, the consume parks the
     continuation and fires nothing. The premises are wf (the counter
     exceeds every order), INV, and a cap above 0 (open_plan opens no plan
     at a cap of 0). *)
  Theorem stack_head_blocks :
    (forall st rank v pers w,
       0 < cap -> ords_below (bump_ord (s_counter st)) st -> no_pending_match st ->
       waiter_top (s_waiters st) = Some w -> fits (w_pat w) v = false ->
       produce st rank v pers =
         mkOutcome [SStore (mkRecord (bump_ord (s_counter st)) rank v pers)] None) /\
    (forall st rank pat pers peek h,
       0 < cap -> ords_below (bump_ord (s_counter st)) st -> no_pending_match st ->
       record_top (s_records st) = Some h -> fits pat (r_val h) = false ->
       consume st rank pat pers peek =
         mkOutcome [SPark (mkWaiter (bump_ord (s_counter st)) rank pat pers peek)] None).
  Proof.
    split.
    - intros st rank v pers w Hcap Hb Hinv Hw Hfit. unfold produce.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + cbn beta. cbn [offer_loop]. cbn [bumped s_waiters]. rewrite Hw.
        cbn [r_val]. rewrite Hfit. reflexivity.
      + rewrite store_after_bump. unfold no_pending_match. cbn [s_records s_waiters].
        unfold record_top. rewrite top_front_max.
        * rewrite Hw. exact Hfit.
        * intros h Hh. left. cbn [r_ord]. exact (proj1 Hb h Hh).
    - intros st rank pat pers peek h Hcap Hb Hinv Hh Hfit. unfold consume.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + cbn beta. cbn [receive_loop]. cbn [bumped s_records]. rewrite Hh.
        cbn [w_pat]. rewrite Hfit. reflexivity.
      + rewrite park_after_bump. unfold no_pending_match. cbn [s_records s_waiters].
        rewrite Hh. unfold waiter_top. rewrite top_front_max.
        * exact Hfit.
        * intros w Hw. left. cbn [w_ord]. exact (proj2 Hb w Hw).
  Qed.

  (* The re-store of a peeked top record puts it back in front, and the
     records are a permutation of the records before the peek. *)
  Lemma drop_record_perm :
    forall l h, ords_distinct r_ord l -> In h l -> Permutation (h :: drop_record h l) l.
  Proof.
    induction l as [| x rest IH]; intros h Hnd Hin; [destruct Hin |].
    unfold drop_record. cbn [drop_key].
    destruct ((r_ord x =? r_ord h) && (r_rank x =? r_rank h)) eqn:Ek.
    - apply andb_true_iff in Ek as [Eo _]. apply Nat.eqb_eq in Eo.
      assert (Hx : x = h).
      { apply (ords_distinct_inj record r_ord (x :: rest) x h Hnd); [left; reflexivity | exact Hin | exact Eo]. }
      subst x. reflexivity.
    - destruct Hin as [Hin | Hin].
      + subst x. rewrite !Nat.eqb_refl in Ek. discriminate.
      + unfold ords_distinct in Hnd. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [_ Hnd].
        apply (perm_trans (perm_swap x h (drop_record h rest))).
        apply perm_skip. apply IH; [exact Hnd | exact Hin].
  Qed.

  (* Headline 11. A peek needs no settle (decision 45, Rule E). A peek of a
     top record that is not persistent fires, removes the record and stores
     it again at its order before any settle step. The tops are then the
     tops before the peek, so the settle after the plan stages nothing: no
     buried entry is exposed. The records are a permutation of the records
     before, the waiters are unchanged, and INV holds. The cap is above 0,
     so open_plan opens the plan. *)
  Theorem stack_peek_restore_needs_no_settle :
    forall st rank pat h,
      0 < cap ->
      ords_distinct r_ord (s_records st) -> no_pending_match st ->
      record_top (s_records st) = Some h -> r_pers h = false -> fits pat (r_val h) = true ->
      let k := mkWaiter (bump_ord (s_counter st)) rank pat false true in
      consume st rank pat false true = mkOutcome [SFireConsume k h; SRestore h] None /\
      Permutation (s_records (after st (consume st rank pat false true))) (s_records st) /\
      s_waiters (after st (consume st rank pat false true)) = s_waiters st /\
      no_pending_match (after st (consume st rank pat false true)).
  Proof.
    intros st rank pat h Hcap Hnd Hinv Hh Hpers Hfit k.
    destruct (top_spec record r_ord r_rank _ h Hh) as [Hin Hmax].
    assert (Hrun : run (bumped st) [SFireConsume k h; SRestore h] =
                   mkS (h :: drop_record h (s_records st)) (s_waiters st) (bump_next (s_counter st))).
    { cbn [run fold_left apply bumped s_records s_waiters s_counter]. rewrite Hpers. reflexivity. }
    assert (Hafter : no_pending_match (run (bumped st) [SFireConsume k h; SRestore h])).
    { rewrite Hrun. unfold no_pending_match. cbn [s_records s_waiters].
      unfold record_top. rewrite top_front_max.
      - unfold no_pending_match in Hinv. rewrite Hh in Hinv. exact Hinv.
      - intros x Hx. apply Hmax. unfold drop_record in Hx. eapply drop_key_incl. exact Hx. }
    assert (Hout : consume st rank pat false true = mkOutcome [SFireConsume k h; SRestore h] None).
    { unfold consume. apply with_settles_quiet; [exact Hcap | exact Hinv | | exact Hafter].
      cbn beta. cbn [receive_loop]. cbn [bumped s_records]. rewrite Hh.
      cbn [w_pat]. rewrite Hfit. unfold consume_fire. cbn [w_peek]. rewrite Hpers.
      cbn [andb negb orb]. unfold after_fire. cbn [negb w_pers]. reflexivity. }
    split; [exact Hout |]. unfold after. rewrite Hout. cbn [o_ops].
    split; [rewrite Hrun; cbn [s_records]; apply drop_record_perm; assumption |].
    split; [rewrite Hrun; reflexivity | exact Hafter].
  Qed.

  (* Headline 9. A re-entry takes its LIFO place (decision 43). A
     re-issued persistent element whose first-round order is below the order
     of the younger top of its side is stored or parked below that top, and
     it fires nothing, although the top of the other side may fit it. An
     element above the top of its side, or on an empty side, plans as a
     fresh element does, from the fire count f of its transaction. The
     premises are INV, distinct orders, the element included (wf' of
     decision 43), and a cap above 0 (open_plan). *)
  Theorem stack_reentry_at_lifo_place :
    (forall st x h,
       0 < cap -> no_pending_match st -> ords_distinct r_ord (x :: s_records st) ->
       record_top (s_records st) = Some h -> r_ord x < r_ord h ->
       reenter_produce st x = mkOutcome [SStore x] None) /\
    (forall st k w,
       0 < cap -> no_pending_match st -> ords_distinct w_ord (k :: s_waiters st) ->
       waiter_top (s_waiters st) = Some w -> w_ord k < w_ord w ->
       reenter_consume st k = mkOutcome [SPark k] None) /\
    (forall f s x,
       match record_top (s_records s) with Some h => r_ord h < r_ord x | None => True end ->
       reenter_offer x f s = offer_loop (S cap) f s x) /\
    (forall f s k,
       match waiter_top (s_waiters s) with Some w => w_ord w < w_ord k | None => True end ->
       reenter_receive k f s = receive_loop (S cap) f s k).
  Proof.
    split; [| split; [| split]].
    - intros st x h Hcap Hinv Hnd Hh Hlt. unfold reenter_produce.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + cbn beta. unfold reenter_offer. cbn [bumped s_records]. rewrite Hh.
        destruct (r_ord h <? r_ord x) eqn:E; [apply Nat.ltb_lt in E; lia | reflexivity].
      + destruct (top_spec record r_ord r_rank _ h Hh) as [Hin Hmax].
        assert (Htop : record_top (x :: s_records st) = Some h).
        { apply top_is_max; [exact Hnd | right; exact Hin |].
          intros y [Hy | Hy]; [subst y; left; exact Hlt | exact (Hmax y Hy)]. }
        rewrite store_after_bump. unfold no_pending_match. cbn [s_records s_waiters].
        rewrite Htop. unfold no_pending_match in Hinv. rewrite Hh in Hinv. exact Hinv.
    - intros st k w Hcap Hinv Hnd Hw Hlt. unfold reenter_consume.
      apply with_settles_quiet; [exact Hcap | exact Hinv | |].
      + cbn beta. unfold reenter_receive. cbn [bumped s_waiters]. rewrite Hw.
        destruct (w_ord w <? w_ord k) eqn:E; [apply Nat.ltb_lt in E; lia | reflexivity].
      + destruct (top_spec waiter w_ord w_rank _ w Hw) as [Hin Hmax].
        assert (Htop : waiter_top (k :: s_waiters st) = Some w).
        { apply top_is_max; [exact Hnd | right; exact Hin |].
          intros y [Hy | Hy]; [subst y; left; exact Hlt | exact (Hmax y Hy)]. }
        rewrite park_after_bump. unfold no_pending_match. cbn [s_records s_waiters].
        rewrite Htop. unfold no_pending_match in Hinv.
        destruct (record_top (s_records st)) as [h |]; [| exact I].
        rewrite Hw in Hinv. exact Hinv.
    - intros f s x H. unfold reenter_offer. destruct (record_top (s_records s)) as [h |]; [| reflexivity].
      destruct (r_ord h <? r_ord x) eqn:E; [reflexivity | apply Nat.ltb_ge in E; lia].
    - intros f s k H. unfold reenter_receive. destruct (waiter_top (s_waiters s)) as [w |]; [| reflexivity].
      destruct (w_ord w <? w_ord k) eqn:E; [reflexivity | apply Nat.ltb_ge in E; lia].
  Qed.

  (* ── 10. The re-issue keeps the first-round order (decision 43) ──────── *)

  (* Every re-issue of an operation carries the order rule of decision 43.
     This holds after the settle before the plan, at the plan-open check,
     and in the plan. The settle after the plan re-issues a settle job. *)
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
    - destruct (next_fire st x) as [[w r0] |]; [| exact (Hland x r H)].
      destruct (at_cap fired); [exact (Hland x r H) |].
      assert (Hstep : In (SStore r) (lift_ops x r0 ++ [SFireProduce w r0]) -> False).
      { unfold lift_ops. destruct x; [| destruct (r_pers r0)]; cbn [app In];
          intros Hin; repeat (destruct Hin as [Hin | Hin]; [discriminate |]); exact Hin. }
      destruct (after_fire (S fired) (negb (r_pers r0) && negb (w_peek w)) (w_pers w));
        cbn [z_ops] in H; apply in_app_or in H as [H | H];
        solve [exact (Hstep H) | exact (IH _ _ _ _ H) | exact (Hland _ _ H)].
  Qed.

  (* The offer loop stores only its own datum. *)
  Lemma offer_loop_stores :
    forall fuel fired st r r', In (SStore r') (p_ops (offer_loop fuel fired st r)) -> r' = r.
  Proof.
    induction fuel as [| fuel IH]; intros fired st r r' H; cbn [offer_loop] in H; [destruct H |].
    destruct (waiter_top (s_waiters st)) as [w |];
      [| destruct H as [H | []]; injection H as <-; reflexivity].
    destruct (fits (w_pat w) (r_val r));
      [| destruct H as [H | []]; injection H as <-; reflexivity].
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [p_ops In] in H; destruct H as [H | H]; try discriminate; try destruct H.
    exact (IH _ _ _ _ H).
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

  (* Headline 10. A re-issue keeps the first-round order (decision 43). A
     re-issued operation carries the order of its first round exactly when
     it is persistent: a fresh produce or consume carries the order of its
     bump, and a re-entry carries its first-round order again. Every round
     bumps the counter once. A re-entered datum stores only itself, so its
     record and its identity are the same in every round. *)
  Theorem stack_reissue_keeps_ord :
    (forall st rank v pers c ca,
       o_next (produce st rank v pers) = Some (NextOperation c ca) ->
       ca = carried pers (bump_ord (s_counter st))) /\
    (forall st rank pat pers peek c ca,
       o_next (consume st rank pat pers peek) = Some (NextOperation c ca) ->
       ca = carried pers (bump_ord (s_counter st))) /\
    (forall st x c ca,
       r_pers x = true -> o_next (reenter_produce st x) = Some (NextOperation c ca) ->
       ca = Some (r_ord x)) /\
    (forall st k c ca,
       w_pers k = true -> o_next (reenter_consume st k) = Some (NextOperation c ca) ->
       ca = Some (w_ord k)) /\
    (forall st o, s_counter (after st o) = Some (S (bump_ord (s_counter st)))) /\
    (forall st x r, In (SStore r) (o_ops (reenter_produce st x)) -> r = x).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros st rank v pers c ca H. exact (with_settles_carried _ _ _ _ _ _ H).
    - intros st rank pat pers peek c ca H. exact (with_settles_carried _ _ _ _ _ _ H).
    - intros st x c ca Hp H. rewrite (with_settles_carried _ _ _ _ _ _ H). unfold carried. rewrite Hp. reflexivity.
    - intros st k c ca Hp H. rewrite (with_settles_carried _ _ _ _ _ _ H). unfold carried. rewrite Hp. reflexivity.
    - intros st o. unfold after. rewrite run_counter. reflexivity.
    - intros st x r H. unfold reenter_produce in H. apply with_settles_stores in H.
      destruct H as [f H]. unfold reenter_offer in H.
      destruct (record_top (s_records (run (bumped st) (z_ops (settle_of (bumped st)))))) as [h |];
        [destruct (r_ord h <? r_ord x) |];
        solve [exact (offer_loop_stores _ _ _ _ _ H) | destruct H as [H | []]; injection H as <-; reflexivity].
  Qed.

  (* ── 11. Termination of the settle ───────────────────────────────────── *)

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
        by (apply (ords_distinct_inj E ord (y :: rest) y h Hnd0); [left; reflexivity | exact Hin | exact Eo]).
      subst y. cbn [filter]. destruct (pers h); cbn [negb length]; lia.
    - destruct Hin as [Hin | Hin]; [subst y; rewrite !Nat.eqb_refl in Ek; discriminate |].
      cbn [filter]. specialize (IH h Hnd Hin). destruct (pers y); cbn [negb length]; lia.
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
          by (apply (ords_distinct_inj E ord (y :: rest) y h Hnd0); [left; reflexivity | right; exact Hin | exact H]).
        subst y. rewrite !Nat.eqb_refl in Ek. discriminate.
      + exact (IH h Hnd Hin H).
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

  (* The invariant of the settle loop: the orders of each side are distinct,
     and a record in flight is not persistent and its order is not stored. *)
  Definition settle_inv (st : sstate) (x : option record) : Prop :=
    ords_distinct r_ord (s_records st) /\ ords_distinct w_ord (s_waiters st) /\
    match x with
    | Some r => r_pers r = false /\ ~ In (r_ord r) (map r_ord (s_records st))
    | None => True
    end.

  Lemma settle_step_waiters :
    forall st x w r,
      s_waiters (run st (lift_ops x r ++ [SFireProduce w r])) =
      if w_pers w then s_waiters st else drop_waiter w (s_waiters st).
  Proof.
    intros st x w r. unfold lift_ops.
    destruct x as [r0 |]; [| destruct (r_pers r)]; cbn [app run fold_left apply];
      destruct (w_pers w); reflexivity.
  Qed.

  Lemma settle_step_inv :
    forall st x w r spent,
      settle_inv st x -> next_fire st x = Some (w, r) ->
      settle_inv (run st (lift_ops x r ++ [SFireProduce w r])) (flight_after x r spent).
  Proof.
    intros st x w r spent [Hr [Hw Hx]] Hnf.
    pose proof (next_fire_some st x w r Hnf) as [_ [_ Hrx]].
    split; [| split].
    - rewrite settle_step_records. destruct x; [exact Hr |].
      destruct (r_pers r); [exact Hr | apply drop_key_distinct; exact Hr].
    - rewrite settle_step_waiters. destruct (w_pers w); [exact Hw | apply drop_key_distinct; exact Hw].
    - destruct (flight_after x r spent) as [r1 |] eqn:Ef; [| exact I].
      destruct (flight_after_some x r spent r1 Ef) as [Hr1 [Hfly _]]. subst r1.
      rewrite settle_step_records. destruct x as [r0 |].
      + subst r0. exact Hx.
      + unfold flying in Hfly. destruct (r_pers r) eqn:Ep; [discriminate |].
        split; [reflexivity |]. apply drop_key_absent; [exact Hr |].
        exact (proj1 (top_spec record r_ord r_rank _ r Hrx)).
  Qed.

  Lemma land_measure :
    forall st x, settle_inv st x -> measure (run st (land x)) = measure st + flight_measure x.
  Proof.
    intros st [r |] [_ [_ Hx]]; cbn [land run fold_left apply flight_measure];
      [| lia].
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

  (* A fire of the settle lowers μ (the record in flight counted), unless it
     is a divergent pair, which keeps it. *)
  Lemma settle_step_measure :
    forall st x w r,
      settle_inv st x -> next_fire st x = Some (w, r) ->
      let spent := negb (r_pers r) && negb (w_peek w) in
      measure (run st (lift_ops x r ++ [SFireProduce w r])) + flight_measure (flight_after x r spent)
        + (if negb spent && w_pers w then 0 else 1)
      <= measure st + flight_measure x.
  Proof.
    intros st x w r [Hr [Hw Hx]] Hnf spent.
    pose proof (next_fire_some st x w r Hnf) as [Hwt [_ Hrx]].
    pose proof (drop_waiter_count (s_waiters st) w Hw
                  (proj1 (top_spec waiter w_ord w_rank _ w Hwt))) as Cw.
    pose proof (flight_measure_after x r spent) as Hfm.
    unfold measure. rewrite settle_step_records, settle_step_waiters.
    unfold spent, flying in *.
    destruct x as [r0 |].
    - subst r0. destruct Hx as [Hp _]. rewrite Hp in *. revert Cw Hfm.
      destruct (w_pers w), (w_peek w); cbn [negb andb flight_measure]; intros Cw Hfm; lia.
    - pose proof (drop_record_count (s_records st) r Hr
                    (proj1 (top_spec record r_ord r_rank _ r Hrx))) as Cr.
      revert Cw Cr Hfm.
      destruct (r_pers r), (w_pers w), (w_peek w); cbn [negb andb flight_measure];
        intros Cw Cr Hfm; lia.
  Qed.

  (* The bounds of the settle loop, for every fuel. *)
  Lemma settle_loop_bounds :
    forall fuel fired st x,
      settle_inv st x -> fired <= cap -> S cap <= fired + fuel ->
      let z := settle_loop fuel fired st x in
      fired + fires (z_ops z) <= cap /\
      fires (z_ops z) + measure (run st (z_ops z)) <= measure st + flight_measure x + divergent_end (z_end z) /\
      (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap) /\
      (z_end z = SettleReissue ReissueDivergent ->
         exists w r, In (SFireProduce w r) (z_ops z) /\ w_pers w = true).
  Proof.
    assert (Hland0 : forall x, fires (land x) = 0) by (intros [r |]; reflexivity).
    induction fuel as [| fuel IH]; intros fired st x Hinv Hle Hfuel z.
    - exfalso. lia.
    - subst z. cbn [settle_loop].
      destruct (next_fire st x) as [[w r] |] eqn:Hnf.
      2: { cbn [z_ops z_end divergent_end]. rewrite Hland0, (land_measure st x Hinv).
           split; [lia | split; [lia | split; intros H; discriminate]]. }
      destruct (at_cap fired) eqn:Hcap.
      { unfold at_cap in Hcap. apply Nat.leb_le in Hcap. cbn [z_ops z_end divergent_end].
        rewrite Hland0, (land_measure st x Hinv).
        split; [lia | split; [lia | split; [intros _; lia | intros H; discriminate]]]. }
      unfold at_cap in Hcap. apply Nat.leb_gt in Hcap.
      pose proof (settle_step_measure st x w r Hinv Hnf) as Hm. cbn zeta in Hm.
      set (spent := negb (r_pers r) && negb (w_peek w)) in *.
      set (ops := lift_ops x r ++ [SFireProduce w r]) in *.
      assert (Hops : fires ops = 1).
      { unfold ops, lift_ops. destruct x; [| destruct (r_pers r)]; reflexivity. }
      assert (Hinv1 : settle_inv (run st ops) (flight_after x r spent))
        by (apply settle_step_inv; assumption).
      assert (Hin : In (SFireProduce w r) ops) by (unfold ops; apply in_or_app; right; left; reflexivity).
      unfold after_fire. destruct spent eqn:Es; cbn [negb andb] in Hm.
      + (* FDone: the record of the fire is spent, and the settle goes on *)
        cbn [z_ops z_end]. specialize (IH (S fired) (run st ops) (flight_after x r true) Hinv1 ltac:(lia) ltac:(lia)).
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

  (* ── 11a. One fire count for each transaction (I-21) ───────────────── *)

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

  Lemma fires_cons :
    forall op rest, fires (op :: rest) = (if is_fire op then 1 else 0) + fires rest.
  Proof. intros op rest. unfold fires. cbn [filter]. destruct (is_fire op); reflexivity. Qed.

  Lemma consume_fire_fires : forall k h, fires (consume_fire k h) = 1.
  Proof. intros k h. unfold consume_fire. destruct (w_peek k && negb (r_pers h)); reflexivity. Qed.

  (* The cap bound of a settle that starts after fired fires of its
     transaction. It needs no premise on the view: the cap check at the top
     of each round stops the settle at the cap. *)
  Lemma settle_loop_cap :
    forall fuel fired st x,
      fired <= cap -> S cap <= fired + fuel ->
      let z := settle_loop fuel fired st x in
      fired + fires (z_ops z) <= cap /\
      (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap).
  Proof.
    assert (Hland0 : forall x, fires (land x) = 0) by (intros [r |]; reflexivity).
    induction fuel as [| fuel IH]; intros fired st x Hle Hfuel z; [exfalso; lia |].
    subst z. cbn [settle_loop].
    destruct (next_fire st x) as [[w r] |].
    2: { cbn [z_ops z_end]. rewrite Hland0. split; [lia | intros H; discriminate]. }
    destruct (at_cap fired) eqn:Hcap.
    { unfold at_cap in Hcap. apply Nat.leb_le in Hcap. cbn [z_ops z_end]. rewrite Hland0.
      split; [lia | intros _; lia]. }
    unfold at_cap in Hcap. apply Nat.leb_gt in Hcap.
    assert (Hops : fires (lift_ops x r ++ [SFireProduce w r]) = 1).
    { unfold lift_ops. destruct x; [| destruct (r_pers r)]; reflexivity. }
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

  (* The bound of a plan that starts after f fires of its transaction: the
     transaction stays at or below the cap, and a plan that stops at the
     cap brings it to exactly the cap. *)
  Definition plan_bounded (f : nat) (p : planned) : Prop :=
    f + fires (p_ops p) <= cap /\
    (p_end p = PReissue ReissueRoundCap -> f + fires (p_ops p) = cap).

  Lemma offer_loop_cap :
    forall fuel fired st r,
      fired < cap -> cap <= fired + fuel -> plan_bounded fired (offer_loop fuel fired st r).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hlt Hfuel; [exfalso; lia |].
    unfold plan_bounded. cbn [offer_loop].
    destruct (waiter_top (s_waiters st)) as [w |];
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    destruct (fits (w_pat w) (r_val r));
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)) eqn:Ea;
      cbn [p_ops p_end].
    - cbn [fires filter is_fire length]. split; [lia | intros H; discriminate].
    - apply after_fire_continue in Ea.
      destruct (IH (S fired) (apply st (SFireProduce w r)) r Ea ltac:(lia)) as [I1 I2].
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
    destruct (record_top (s_records st)) as [h |];
      [| cbn [p_ops p_end fires filter is_fire length]; split; [lia | intros H; discriminate]].
    destruct (fits (w_pat k) (r_val h));
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

  (* Headline 6. The settle terminates, and every transaction makes at most
     cap fires (I-21). A settle that starts after fired fires of its
     transaction makes at most cap - fired fires. Every fire lowers μ, the
     stored records and waiters that are not persistent with the record in
     flight, except a final divergent fire. So the fires are at most μ, plus
     one for a divergent pair. A settle that stops at the cap brings its
     transaction to exactly cap fires. A divergent pair has a persistent
     waiter, and a settle that ends Quiescent leaves INV. The premise of
     these settle bounds is wf: distinct orders on each side.
     A produce, a consume, a settle job and a re-entry each make at most
     cap fires with their settles, with no premise. A transaction that
     re-issues at the cap made exactly cap fires. *)
  Theorem stack_settle_terminates :
    (forall fired st,
       ords_distinct r_ord (s_records st) -> ords_distinct w_ord (s_waiters st) -> fired <= cap ->
       let z := settle_at fired st in
       fired + fires (z_ops z) <= cap /\
       fires (z_ops z) <= measure st + divergent_end (z_end z) /\
       (z_end z = SettleReissue ReissueRoundCap -> fired + fires (z_ops z) = cap) /\
       (z_end z = SettleReissue ReissueDivergent ->
          exists w r, In (SFireProduce w r) (z_ops z) /\ w_pers w = true) /\
       (z_end z = Quiescent -> no_pending_match (run st (z_ops z)))) /\
    (forall st rank v pers, txn_bounded (produce st rank v pers)) /\
    (forall st rank pat pers peek, txn_bounded (consume st rank pat pers peek)) /\
    (forall st, txn_bounded (settle_job st)) /\
    (forall st x, txn_bounded (reenter_produce st x)) /\
    (forall st k, txn_bounded (reenter_consume st k)).
  Proof.
    (* The bound of with_settles for every plan that keeps the plan bound
       from every count below the cap. The plan-open check gives the plan a
       count below the cap. The settle after the plan continues the count
       of the plan. *)
    assert (Hws : forall st pers o plan,
               (forall f s, f < cap -> plan_bounded f (plan f s)) ->
               txn_bounded (with_settles st pers o plan)).
    { intros st pers o plan Hplan. unfold with_settles, txn_bounded, settle_of.
      pose proof (settle_loop_cap (S cap) 0 st None ltac:(lia) ltac:(lia)) as B.
      cbn zeta in B. destruct B as [B1 B2].
      destruct (z_end (settle_loop (S cap) 0 st None)) as [| c] eqn:Eb.
      2: { cbn [o_ops o_next]. split; [lia |].
           intros n Hn Hc. injection Hn as <-. cbn [next_cause] in Hc. subst c.
           try rewrite Eb in B2. specialize (B2 eq_refl). lia. }
      set (B := z_ops (settle_loop (S cap) 0 st None)) in *.
      destruct (plan_at_cap (fires B)) eqn:Ec.
      { apply plan_at_cap_true in Ec. cbn [o_ops o_next]. split; [lia | intros n Hn _; lia]. }
      unfold plan_at_cap in Ec. apply Nat.leb_gt in Ec.
      destruct (Hplan (fires B) (run st B) Ec) as [P1 P2].
      set (Pl := plan (fires B) (run st B)) in *.
      destruct (p_end Pl) as [| c] eqn:Ep; cbn [o_ops o_next].
      - lazymatch goal with
        | |- context [settle_at ?g ?s] =>
            pose proof (settle_loop_cap (S cap) g s None ltac:(lia) ltac:(lia)) as A;
            cbn zeta in A; destruct A as [A1 A2];
            unfold settle_at; rewrite !fires_app; split; [lia |];
            intros n Hn Hc;
            destruct (z_end (settle_loop (S cap) g s None)) as [| c'] eqn:Ea;
            cbn [settle_next] in Hn; [discriminate |];
            injection Hn as <-; cbn [next_cause] in Hc; subst c';
            try rewrite Ea in A2; specialize (A2 eq_refl); lia
        end.
      - rewrite fires_app. split; [lia |].
        intros n Hn Hc. injection Hn as <-. cbn [next_cause] in Hc. subst c.
        try rewrite Ep in P2. specialize (P2 eq_refl). lia. }
    split; [| split; [| split; [| split; [| split]]]].
    - intros fired st Hr Hw Hle z. unfold z, settle_at.
      assert (Hinv : settle_inv st None) by (split; [exact Hr | split; [exact Hw | exact I]]).
      pose proof (settle_loop_bounds (S cap) fired st None Hinv Hle ltac:(lia)) as B.
      cbn zeta in B. destruct B as [B1 [B2 [B3 B4]]]. cbn [flight_measure] in B2.
      split; [lia | split; [lia | split; [exact B3 | split; [exact B4 |]]]].
      intros H. apply settle_quiescent_inv; [exact I | exact H].
    - intros st rank v pers. unfold produce. apply Hws. intros f s Hf.
      exact (offer_loop_cap (S cap) f s _ Hf ltac:(lia)).
    - intros st rank pat pers peek. unfold consume. apply Hws. intros f s Hf.
      exact (receive_loop_cap (S cap) f s _ Hf ltac:(lia)).
    - intros st. unfold settle_job, txn_bounded, settle_of.
      pose proof (settle_loop_cap (S cap) 0 (bumped st) None ltac:(lia) ltac:(lia)) as B.
      cbn zeta in B. destruct B as [B1 B2]. cbn [o_ops o_next]. split; [lia |].
      intros n Hn Hc.
      destruct (z_end (settle_loop (S cap) 0 (bumped st) None)) as [| c] eqn:Ea;
        cbn [settle_next] in Hn; [discriminate |].
      injection Hn as <-. cbn [next_cause] in Hc. subst c.
      try rewrite Ea in B2. specialize (B2 eq_refl). lia.
    - intros st x. unfold reenter_produce. apply Hws. intros f s Hf. unfold reenter_offer.
      destruct (record_top (s_records s)) as [h |]; [destruct (r_ord h <? r_ord x) |];
        first [exact (offer_loop_cap (S cap) f s x Hf ltac:(lia))
              | unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
                split; [lia | intros H; discriminate]].
    - intros st k. unfold reenter_consume. apply Hws. intros f s Hf. unfold reenter_receive.
      destruct (waiter_top (s_waiters s)) as [w |]; [destruct (w_ord w <? w_ord k) |];
        first [exact (receive_loop_cap (S cap) f s k Hf ltac:(lia))
              | unfold plan_bounded; cbn [p_ops p_end fires filter is_fire length];
                split; [lia | intros H; discriminate]].
  Qed.

  (* ── 11b. A settle loses no record ─────────────────────────────────── *)

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
        by (apply (ords_distinct_inj record r_ord (y :: rest) y r Hnd0); [left; reflexivity | exact Hr | exact Eo]).
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

  (* The loop form: a record that is stored or in flight stays, or a fire
     of a waiter that does not peek spends it. *)
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
    - destruct (next_fire st x) as [[w r] |] eqn:Hnf; [| left; apply land_keeps; exact Hh].
      destruct (at_cap fired); [left; apply land_keeps; exact Hh |].
      pose proof (next_fire_some st x w r Hnf) as [_ [_ Hrx]].
      set (spent := negb (r_pers r) && negb (w_peek w)).
      set (ops := lift_ops x r ++ [SFireProduce w r]).
      assert (Hinv1 : settle_inv (run st ops) (flight_after x r spent))
        by (apply settle_step_inv; assumption).
      assert (Hfire : In (SFireProduce w r) ops)
        by (unfold ops; apply in_or_app; right; left; reflexivity).
      (* After the step, h is stored, in flight, or spent by this fire. *)
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
          pose proof (proj1 (top_spec record r_ord r_rank _ r Hrx)) as Hrin.
          destruct (Nat.eq_dec (r_ord h) (r_ord r)) as [Eo | Ene].
          + assert (Heq : h = r)
              by exact (ords_distinct_inj record r_ord (s_records st) h r Hr Hh Hrin Eo).
            subst h. unfold flight_after, flying, spent. rewrite Ep. cbn [negb andb].
            destruct (w_peek w) eqn:Ek; cbn [negb].
            * right. left. reflexivity.
            * right. right. split; [reflexivity | split; [exact Hfire | reflexivity]].
          + left. apply drop_record_keeps; [exact Hr | exact Hh | exact Hrin |].
            intros Heq. subst h. contradiction. }
      destruct (after_fire (S fired) spent (w_pers w)); cbn [z_ops].
      + destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
        * destruct (IH (S fired) _ _ Hinv1 h (or_introl Hs)) as [Hl | [Hp [w1 [Hin1 Hk1]]]];
            [left; rewrite run_app; exact Hl |].
          right. split; [exact Hp |]. exists w1. split; [apply in_or_app; right; exact Hin1 | exact Hk1].
        * destruct (IH (S fired) _ _ Hinv1 h (or_intror Hs)) as [Hl | [Hp [w1 [Hin1 Hk1]]]];
            [left; rewrite run_app; exact Hl |].
          right. split; [exact Hp |]. exists w1. split; [apply in_or_app; right; exact Hin1 | exact Hk1].
        * right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
      + destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
        * destruct (IH (S fired) _ _ Hinv1 h (or_introl Hs)) as [Hl | [Hp [w1 [Hin1 Hk1]]]];
            [left; rewrite run_app; exact Hl |].
          right. split; [exact Hp |]. exists w1. split; [apply in_or_app; right; exact Hin1 | exact Hk1].
        * destruct (IH (S fired) _ _ Hinv1 h (or_intror Hs)) as [Hl | [Hp [w1 [Hin1 Hk1]]]];
            [left; rewrite run_app; exact Hl |].
          right. split; [exact Hp |]. exists w1. split; [apply in_or_app; right; exact Hin1 | exact Hk1].
        * right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
      + destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
        * left. rewrite run_app. apply land_keeps. left. exact Hs.
        * left. rewrite run_app. apply land_keeps. right. exact Hs.
        * right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
      + destruct Hstep as [Hs | [Hs | [Hp [Hin Hk]]]].
        * left. rewrite run_app. apply land_keeps. left. exact Hs.
        * left. rewrite run_app. apply land_keeps. right. exact Hs.
        * right. split; [exact Hp |]. exists w. split; [apply in_or_app; left; exact Hin | exact Hk].
  Qed.

  (* Headline 13. A settle loses no record (decisions 30 and 45). A stored
     record leaves the store during a settle only when a fire of a waiter
     that does not peek spends it, and then the record is not persistent. A
     lifted record whose fire a waiter that peeks did not spend stays in
     flight, and the settle stores it again with its bytes when it has no
     further fire or when the settle re-issues. This holds from every fire
     count of the transaction (I-21): settle_of is settle_at 0, and the
     settle after the plan is settle_at f. The premise is wf: distinct
     orders on each side. *)
  Theorem stack_settle_keeps_unspent_records :
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

  (* ── 12. The plans ignore the list order ────────────────────────────── *)

  Lemma ords_distinct_perm :
    forall (E : Type) (ord : E -> nat) l1 l2,
      Permutation l1 l2 -> ords_distinct ord l1 -> ords_distinct ord l2.
  Proof.
    intros E ord l1 l2 Hp Hnd. unfold ords_distinct in *.
    eapply Permutation_NoDup; [apply Permutation_map; exact Hp | exact Hnd].
  Qed.

  (* Permuted lists with distinct orders have one top. *)
  Lemma top_perm :
    forall (E : Type) (ord rank : E -> nat) l1 l2,
      Permutation l1 l2 -> ords_distinct ord l1 -> top ord rank l1 = top ord rank l2.
  Proof.
    intros E ord rank l1 l2 Hp Hnd. destruct l1 as [| a rest].
    - apply Permutation_nil in Hp. subst l2. reflexivity.
    - destruct (top_some E ord rank (a :: rest) ltac:(discriminate)) as [t Ht]. rewrite Ht.
      destruct (top_spec E ord rank _ t Ht) as [Hin Hmax].
      symmetry. apply top_is_max.
      + eapply ords_distinct_perm; eassumption.
      + eapply Permutation_in; eassumption.
      + intros x Hx. apply Hmax. eapply Permutation_in; [apply Permutation_sym; exact Hp | exact Hx].
  Qed.

  Lemma filter_all :
    forall (A : Type) (f : A -> bool) l, (forall x, In x l -> f x = true) -> filter f l = l.
  Proof.
    intros A f l. induction l as [| y rest IH]; intros H; [reflexivity |].
    cbn [filter]. rewrite (H y (or_introl eq_refl)). f_equal.
    apply IH. intros x Hx. apply H. right. exact Hx.
  Qed.

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

  (* With distinct orders, the removal of the first entry with a key is the
     removal of every entry with that key. *)
  Lemma drop_key_filter :
    forall (E : Type) (ord rank : E -> nat) o k l,
      ords_distinct ord l ->
      drop_key ord rank o k l = filter (fun x => negb ((ord x =? o) && (rank x =? k))) l.
  Proof.
    intros E ord rank o k l. induction l as [| y rest IH]; intros Hnd; [reflexivity |].
    unfold ords_distinct in Hnd. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    cbn [drop_key filter].
    destruct ((ord y =? o) && (rank y =? k)) eqn:Ey; cbn [negb].
    - symmetry. apply filter_all. intros z Hz. apply andb_true_iff in Ey as [Eo _].
      apply Nat.eqb_eq in Eo.
      destruct ((ord z =? o) && (rank z =? k)) eqn:Ez; [| reflexivity].
      apply andb_true_iff in Ez as [Ezo _]. apply Nat.eqb_eq in Ezo.
      exfalso. apply Hnot. rewrite Eo, <- Ezo. apply in_map. exact Hz.
    - f_equal. apply IH. exact Hnd.
  Qed.

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

  (* Two views with the same entries in other list orders. *)
  Definition s_equiv (st1 st2 : sstate) : Prop :=
    Permutation (s_records st1) (s_records st2) /\ Permutation (s_waiters st1) (s_waiters st2) /\
    s_counter st1 = s_counter st2.

  (* Distinct orders on each side. *)
  Definition dist (st : sstate) : Prop :=
    ords_distinct r_ord (s_records st) /\ ords_distinct w_ord (s_waiters st).

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

  Lemma tops_equiv :
    forall st1 st2, s_equiv st1 st2 -> dist st1 ->
      record_top (s_records st1) = record_top (s_records st2) /\
      waiter_top (s_waiters st1) = waiter_top (s_waiters st2).
  Proof.
    intros st1 st2 [Hr [Hw _]] [Dr Dw]. unfold record_top, waiter_top.
    split; apply top_perm; assumption.
  Qed.

  Lemma next_fire_equiv :
    forall st1 st2 x, s_equiv st1 st2 -> dist st1 -> next_fire st1 x = next_fire st2 x.
  Proof.
    intros st1 st2 x He Hd. destruct (tops_equiv st1 st2 He Hd) as [Er Ew].
    unfold next_fire. rewrite Ew, Er. reflexivity.
  Qed.

  Lemma dist_cons_record :
    forall r l, ords_distinct r_ord l -> ~ In (r_ord r) (map r_ord l) -> ords_distinct r_ord (r :: l).
  Proof. intros r l Hnd Hnot. unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption. Qed.

  Lemma dist_cons_waiter :
    forall w l, ords_distinct w_ord l -> ~ In (w_ord w) (map w_ord l) -> ords_distinct w_ord (w :: l).
  Proof. intros w l Hnd Hnot. unfold ords_distinct in *. cbn [map]. apply NoDup_cons; assumption. Qed.

  Lemma land_equiv :
    forall st1 st2 x, s_equiv st1 st2 -> settle_inv st1 x ->
      s_equiv (run st1 (land x)) (run st2 (land x)) /\ dist (run st1 (land x)).
  Proof.
    intros st1 st2 [r |] He [Dr [Dw Hx]]; cbn [land run fold_left apply].
    - destruct He as [Hr [Hw Hc]]. destruct Hx as [_ Hnot].
      split; [split; [apply perm_skip; exact Hr | split; assumption] |].
      split; [apply dist_cons_record; assumption | exact Dw].
    - split; [exact He | split; assumption].
  Qed.

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
    induction fuel as [| fuel IH]; intros fired st1 st2 x He Hinv; cbn [settle_loop].
    - split; [reflexivity |]. exact (land_equiv st1 st2 x He Hinv).
    - assert (Hd : dist st1) by (destruct Hinv as [Dr [Dw _]]; split; assumption).
      rewrite <- (next_fire_equiv st1 st2 x He Hd).
      destruct (next_fire st1 x) as [[w r] |] eqn:Hnf;
        [| split; [reflexivity | exact (land_equiv st1 st2 x He Hinv)]].
      destruct (at_cap fired); [split; [reflexivity | exact (land_equiv st1 st2 x He Hinv)] |].
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
        by (intros spent; apply settle_step_inv; assumption).
      set (spent := negb (r_pers r) && negb (w_peek w)).
      destruct (after_fire (S fired) spent (w_pers w)).
      + destruct (IH (S fired) (run st1 ops) (run st2 ops) (flight_after x r spent) Hops (Hinv1 spent))
          as [Eq [Hequiv Hdist]].
        rewrite <- Eq. cbn [z_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
      + destruct (IH (S fired) (run st1 ops) (run st2 ops) (flight_after x r spent) Hops (Hinv1 spent))
          as [Eq [Hequiv Hdist]].
        rewrite <- Eq. cbn [z_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
      + cbn [z_ops]. rewrite !run_app. split; [reflexivity |].
        apply land_equiv; [exact Hops | apply Hinv1].
      + cbn [z_ops]. rewrite !run_app. split; [reflexivity |].
        apply land_equiv; [exact Hops | apply Hinv1].
  Qed.

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

  Lemma offer_loop_equiv :
    forall fuel fired st1 st2 r,
      s_equiv st1 st2 -> dist st1 -> ~ In (r_ord r) (map r_ord (s_records st1)) ->
      offer_loop fuel fired st1 r = offer_loop fuel fired st2 r /\
      s_equiv (run st1 (p_ops (offer_loop fuel fired st1 r)))
              (run st2 (p_ops (offer_loop fuel fired st1 r))) /\
      dist (run st1 (p_ops (offer_loop fuel fired st1 r))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st1 st2 r He Hd Hfresh; cbn [offer_loop].
    - split; [reflexivity | split; [exact He | exact Hd]].
    - destruct (tops_equiv st1 st2 He Hd) as [_ Ew]. rewrite <- Ew.
      pose proof (store_equiv st1 st2 r He Hd Hfresh) as Hstore.
      destruct (waiter_top (s_waiters st1)) as [w |]; [| split; [reflexivity | exact Hstore]].
      destruct (fits (w_pat w) (r_val r)); [| split; [reflexivity | exact Hstore]].
      destruct (fire_produce_equiv st1 st2 w r He Hd) as [Hfe Hfd].
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
        try (split; [reflexivity | cbn [p_ops run fold_left]; split; assumption]).
      assert (Hfresh1 : ~ In (r_ord r) (map r_ord (s_records (apply st1 (SFireProduce w r)))))
        by (rewrite fire_produce_records; exact Hfresh).
      destruct (IH (S fired) _ _ r Hfe Hfd Hfresh1) as [Eq [Hq Hdd]].
      rewrite <- Eq. cbn [p_ops]. rewrite !run_cons. split; [reflexivity | split; assumption].
  Qed.

  Lemma consume_fire_equiv :
    forall st1 st2 k h,
      s_equiv st1 st2 -> dist st1 -> record_top (s_records st1) = Some h ->
      s_equiv (run st1 (consume_fire k h)) (run st2 (consume_fire k h)) /\
      dist (run st1 (consume_fire k h)).
  Proof.
    intros st1 st2 k h He Hd Hh.
    pose proof (proj1 (top_spec record r_ord r_rank _ h Hh)) as Hin.
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
    - destruct (tops_equiv st1 st2 He Hd) as [Er _]. rewrite <- Er.
      pose proof (park_equiv st1 st2 k He Hd Hfresh) as Hpark.
      destruct (record_top (s_records st1)) as [h |] eqn:Hh; [| split; [reflexivity | exact Hpark]].
      destruct (fits (w_pat k) (r_val h)); [| split; [reflexivity | exact Hpark]].
      destruct (consume_fire_equiv st1 st2 k h He Hd Hh) as [Hfe Hfd].
      destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
        try (split; [reflexivity | split; assumption]).
      assert (Hfresh1 : ~ In (w_ord k) (map w_ord (s_waiters (run st1 (consume_fire k h)))))
        by (rewrite consume_fire_waiters; exact Hfresh).
      destruct (IH (S fired) _ _ k Hfe Hfd Hfresh1) as [Eq [Hq Hdd]].
      rewrite <- Eq. cbn [p_ops]. rewrite !run_app. split; [reflexivity | split; assumption].
  Qed.

  Lemma settle_inv_of_dist : forall st, dist st -> settle_inv st None.
  Proof. intros st [Dr Dw]. split; [exact Dr | split; [exact Dw | exact I]]. Qed.

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
    destruct (settle_loop_equiv (S cap) 0 st1 st2 None He (settle_inv_of_dist st1 Hd))
      as [Eb [Hbe Hbd]].
    unfold with_settles, settle_of in *. rewrite <- Eb.
    set (B := z_ops (settle_loop (S cap) 0 st1 None)) in *.
    destruct (z_end (settle_loop (S cap) 0 st1 None)); [| split; [reflexivity | exact Hbe]].
    destruct (plan_at_cap (fires B)); [split; [reflexivity | exact Hbe] |].
    lazymatch goal with
    | |- context [plan ?f (run st1 B)] =>
        destruct (Hplan f _ _ Hbe Hbd Hfresh) as [Ep [Hpe Hpd]]; rewrite <- Ep;
        set (Pops := p_ops (plan f (run st1 B))) in *;
        destruct (p_end (plan f (run st1 B)))
    end.
    - unfold settle_at.
      lazymatch goal with
      | |- context [settle_loop (S cap) ?g _ None] =>
          destruct (settle_loop_equiv (S cap) g _ _ None Hpe (settle_inv_of_dist _ Hpd))
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

  (* Headline 7. The plans ignore the list order. The hot store inserts at
     the front, and a restart can load another order. Two views with the
     same entries in other list orders stage the same operations in a
     produce, a consume and a settle job, take the same next round, and
     reach views with the same entries. The premises are wf: distinct
     orders on each side and orders below the counter. *)
  Theorem stack_plan_permutation_invariant :
    forall st1 st2,
      s_equiv st1 st2 -> dist st1 -> ords_below (bump_ord (s_counter st1)) st1 ->
      (forall rank v pers,
         produce st1 rank v pers = produce st2 rank v pers /\
         s_equiv (after st1 (produce st1 rank v pers)) (after st2 (produce st2 rank v pers))) /\
      (forall rank pat pers peek,
         consume st1 rank pat pers peek = consume st2 rank pat pers peek /\
         s_equiv (after st1 (consume st1 rank pat pers peek))
                 (after st2 (consume st2 rank pat pers peek))) /\
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
    - intros rank v pers. unfold produce, after. cbn zeta. rewrite <- Hc.
      destruct (with_settles_equiv (bumped st1) (bumped st2) pers (bump_ord (s_counter st1))
                  (fun f s => offer_loop (S cap) f s (mkRecord (bump_ord (s_counter st1)) rank v pers))
                  (fun s => ~ In (bump_ord (s_counter st1)) (map r_ord (s_records s))) Hbe Hbd)
        as [Eq Heq].
      + intros f s1 s2 Hs Hsd Hsf.
        exact (offer_loop_equiv (S cap) f s1 s2 (mkRecord (bump_ord (s_counter st1)) rank v pers)
                 Hs Hsd Hsf).
      + apply below_fresh_record. apply settle_ords_below; [exact Hbb | exact I].
      + split; [exact Eq | rewrite <- Eq; exact Heq].
    - intros rank pat pers peek. unfold consume, after. cbn zeta. rewrite <- Hc.
      destruct (with_settles_equiv (bumped st1) (bumped st2) pers (bump_ord (s_counter st1))
                  (fun f s => receive_loop (S cap) f s
                                (mkWaiter (bump_ord (s_counter st1)) rank pat pers peek))
                  (fun s => ~ In (bump_ord (s_counter st1)) (map w_ord (s_waiters s))) Hbe Hbd)
        as [Eq Heq].
      + intros f s1 s2 Hs Hsd Hsf.
        exact (receive_loop_equiv (S cap) f s1 s2
                 (mkWaiter (bump_ord (s_counter st1)) rank pat pers peek) Hs Hsd Hsf).
      + apply below_fresh_waiter. apply settle_ords_below; [exact Hbb | exact I].
      + split; [exact Eq | rewrite <- Eq; exact Heq].
    - unfold settle_job, after, settle_of.
      destruct (settle_loop_equiv (S cap) 0 (bumped st1) (bumped st2) None Hbe
                  (settle_inv_of_dist _ Hbd)) as [Eq [Heq _]].
      rewrite <- Eq. split; [reflexivity | exact Heq].
  Qed.

  (* ── 13. The Unsafe control: a stack without the settle ───────────────── *)

  (* The produce plan without the settles before and after it (the Unsafe
     control StackNoSettle of StackDiscipline.tla). *)
  Definition produce_without_settle (st : sstate) (rank : nat) (v : V) (pers : bool) : outcome :=
    let o := bump_ord (s_counter st) in
    let p := offer_loop (S cap) 0 (bumped st) (mkRecord o rank v pers) in
    mkOutcome (p_ops p)
      (match p_end p with PDone => None | PReissue c => Some (NextOperation c (carried pers o)) end).
End Stack.

(* ── 14. Headline 8: the settle is needed ───────────────────────────────── *)

(* The counterexample of the P5 design v2, section 3.1. A pattern fits a
   value when they are equal. The receive w2 for 1 parks (ord 0). The
   receive w for 2 parks on top of it (ord 1). The send h of 1 meets the top
   waiter w, which does not fit it, so h is stored (ord 2). *)
Definition ce_w2 : waiter nat := mkWaiter nat 0 0 1 false false.
Definition ce_w : waiter nat := mkWaiter nat 1 0 2 false false.
Definition ce_h : record nat := mkRecord nat 2 0 1 false.
Definition ce_d : record nat := mkRecord nat 3 0 2 false.
Definition ce_empty : sstate nat nat := mkS nat nat [] [] None.
Definition ce_state : sstate nat nat := mkS nat nat [ce_h] [ce_w; ce_w2] (Some 3).

(* Headline 8. A stack without the settle breaks INV. The three operations
   reach ce_state, which satisfies INV. The send d of 2 then fires the top
   waiter w. Without the settle, the top record h and the top waiter w2
   then match: INV fails at the end of a completed operation. With the
   settle after the plan, the stack lifts h and offers it again to w2, and
   INV holds. The cap is 64, FIRE_ROUND_CAP. *)
Theorem stack_without_settle_breaks_inv :
  let s1 := after nat nat ce_empty (consume nat nat Nat.eqb 64 ce_empty 0 1 false false) in
  let s2 := after nat nat s1 (consume nat nat Nat.eqb 64 s1 0 2 false false) in
  after nat nat s2 (produce nat nat Nat.eqb 64 s2 0 1 false) = ce_state /\
  no_pending_match nat nat Nat.eqb ce_state /\
  produce_without_settle nat nat Nat.eqb 64 ce_state 0 2 false =
    mkOutcome nat nat [SFireProduce nat nat ce_w ce_d] None /\
  ~ no_pending_match nat nat Nat.eqb
      (after nat nat ce_state (produce_without_settle nat nat Nat.eqb 64 ce_state 0 2 false)) /\
  produce nat nat Nat.eqb 64 ce_state 0 2 false =
    mkOutcome nat nat
      [SFireProduce nat nat ce_w ce_d; SLift nat nat ce_h; SFireProduce nat nat ce_w2 ce_h] None /\
  no_pending_match nat nat Nat.eqb (after nat nat ce_state (produce nat nat Nat.eqb 64 ce_state 0 2 false)).
Proof.
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [cbv; discriminate |]. split; [reflexivity | exact I].
Qed.

(* ── 15. Headline 12: a settle reserves its reads before it makes them ───── *)

(* The metering rule of DR-88 (decision 42), restated from AgentReadCoverage.v
   so that this module stays standalone. A metered computation is a list of
   events: Reserve u adds u units to the host-work budget, and Read u is
   work of u units that no other step meters. Prefix coverage: every prefix
   reads at most the units that the same prefix has reserved. Each
   host-work dimension satisfies the rule alone, so the model counts one. *)
Inductive event : Type := Reserve (units : nat) | Read (units : nat).

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

(* An entry of one side of a collection in the staged view: the units of the
   decode of its envelope, and the units of a copy of the whole entry. *)
Record entry : Type := mkEntry { envelope : nat; payload : nat }.

Section SettleMetering.
  (* The units of a buffer of n slots, and of one comparison of two orders. *)
  Variable buffer : nat -> nat.
  Variable compare : nat.

  (* Cx::head_waiter or Cx::head_datum with StackDiscipline::head_metered:
     the buffer of the ordered view, the decode of each envelope, one
     comparison for each entry after the first, and the copy of the top. *)
  Definition scan (side : list entry) (top_entry : entry) : list event :=
    step (buffer (length side))
    ++ flat_map step (map envelope side)
    ++ flat_map step (map (fun _ => compare) (tl side))
    ++ step (payload top_entry).

  (* One round of the settle loop that fires (settle.rs and StackView). *)
  Record settle_round : Type := mkRound {
    waiter_side : list entry;   (* the waiters that StackView scans *)
    waiter_head : entry;
    record_side : list entry;   (* the records that StackView scans *)
    record_head : entry;
    in_flight : bool;           (* the record of the fire is in flight: no record scan *)
    decode : nat;               (* the payload decode of the record (unwrap_datum_metered) *)
    matcher : list nat;         (* the reads of get_metered and check_commit_metered *)
    plan_copies : list nat;     (* the copy of the channel and the buffers of the plan *)
    lift : list nat;            (* the match, template, random bytes, copies and buffers of K_lift *)
    unwrap : nat;               (* the decode of the fired continuation, which the round prepays *)
    reoffer : list nat          (* the introduction and the copies of the re-offer *)
  }.

  Definition round_staged (r : settle_round) : list event :=
    scan (waiter_side r) (waiter_head r)
    ++ (if in_flight r then [] else scan (record_side r) (record_head r))
    ++ step (decode r)
    ++ flat_map step (matcher r)
    ++ flat_map step (plan_copies r)
    ++ flat_map step (lift r)
    ++ [Reserve (unwrap r)]
    ++ flat_map step (reoffer r).

  (* After the commit, the outcome decodes each fired continuation. *)
  Definition round_after_commit (r : settle_round) : list event := [Read (unwrap r)].

  (* A settle call: its rounds that fire, then its last check, which is
     self-metered: the scans and the match that find no fire or the cap
     check, and the landing of the record in flight (Cx::restore_on). *)
  Record settle_run : Type := mkRun { rounds : list settle_round; last_check : list nat }.

  Definition settle_staged (x : settle_run) : list event :=
    flat_map round_staged (rounds x) ++ flat_map step (last_check x).

  Definition settle_after_commit (x : settle_run) : list event :=
    flat_map round_after_commit (rounds x).

  Definition settle_trace (x : settle_run) : list event := settle_staged x ++ settle_after_commit x.

  Lemma scan_balanced : forall side top_entry, balanced (scan side top_entry).
  Proof.
    intros side top_entry. unfold scan.
    repeat apply balanced_app; apply steps_balanced || apply (steps_balanced [_]).
  Qed.

  Definition prepaid (r : settle_round) : nat := unwrap r.

  (* The parts of a round other than the prepaid reservation. *)
  Lemma round_parts_balanced : forall r,
    balanced (scan (waiter_side r) (waiter_head r)
              ++ (if in_flight r then [] else scan (record_side r) (record_head r))
              ++ step (decode r) ++ flat_map step (matcher r) ++ flat_map step (plan_copies r)
              ++ flat_map step (lift r)).
  Proof.
    intros r. apply balanced_app; [apply scan_balanced |].
    apply balanced_app; [destruct (in_flight r); [apply balanced_nil | apply scan_balanced] |].
    apply balanced_app; [apply (steps_balanced [decode r]) |].
    apply balanced_app; [apply steps_balanced |].
    apply balanced_app; apply steps_balanced.
  Qed.

  Lemma round_staged_shape : forall r,
    round_staged r =
      (scan (waiter_side r) (waiter_head r)
       ++ (if in_flight r then [] else scan (record_side r) (record_head r))
       ++ step (decode r) ++ flat_map step (matcher r) ++ flat_map step (plan_copies r)
       ++ flat_map step (lift r))
      ++ [Reserve (unwrap r)] ++ flat_map step (reoffer r).
  Proof. intros r. unfold round_staged. rewrite <- !app_assoc. reflexivity. Qed.

  (* A staged round is covered, and it leaves its prepaid units as slack. *)
  Lemma round_staged_covered : forall slack r, covered_from slack (round_staged r).
  Proof.
    intros slack r. rewrite round_staged_shape.
    pose proof (round_parts_balanced r) as [parts_covered parts_equal].
    apply covered_from_app; [apply parts_covered | lia |].
    apply covered_from_app; [apply reserve_covered | unfold total; simpl; lia |].
    apply steps_covered.
  Qed.

  (* A reservation between two balanced segments leaves its units as slack. *)
  Lemma totals_prepaid : forall parts u rest,
    balanced parts -> balanced rest ->
    total read_units (parts ++ [Reserve u] ++ rest) + u =
    total reserved_units (parts ++ [Reserve u] ++ rest).
  Proof.
    intros parts u rest [_ Hp] [_ Hr]. rewrite !total_app, Hp, Hr.
    cbn [total map fold_right read_units reserved_units]. lia.
  Qed.

  Lemma round_staged_totals : forall r,
    total read_units (round_staged r) + prepaid r = total reserved_units (round_staged r).
  Proof.
    intros r. rewrite round_staged_shape. unfold prepaid.
    apply totals_prepaid; [apply round_parts_balanced | apply steps_balanced].
  Qed.

  Lemma rounds_staged_covered : forall slack rs, covered_from slack (flat_map round_staged rs).
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
    cbn [map]. rewrite total_app, IH, sum_cons. unfold round_after_commit, prepaid, total. simpl. lia.
  Qed.

  Lemma after_rounds_reserved : forall rs,
    total reserved_units (flat_map round_after_commit rs) = 0.
  Proof.
    induction rs as [| r rest IH]; [reflexivity |].
    change (flat_map round_after_commit (r :: rest))
      with (round_after_commit r ++ flat_map round_after_commit rest).
    rewrite total_app, IH. unfold round_after_commit, total. simpl. lia.
  Qed.

  (* Headline 12. A settle reserves its reads before it makes them
     (decision 42, DR-88): its trace satisfies prefix coverage, the reads of
     the outcome after the commit included. Each round of the settle scans
     the two tops, decodes the payload, matches, copies the plan, lifts,
     prepays the decode of the fired continuation and stages the re-offer,
     each step self-metered. *)
  Theorem settle_trace_covered : forall x, covered (settle_trace x).
  Proof.
    intros x. unfold covered, settle_trace, settle_staged.
    pose proof (rounds_staged_totals (rounds x)) as Hrounds.
    pose proof (steps_balanced (last_check x)) as [Hlast_cov Hlast_eq].
    rewrite <- app_assoc.
    apply covered_from_app; [apply rounds_staged_covered | lia |].
    apply covered_from_app; [apply Hlast_cov | lia |].
    apply reads_only_covered; unfold settle_after_commit.
    - apply after_rounds_reserved.
    - rewrite after_rounds_read. lia.
  Qed.
End SettleMetering.

Print Assumptions stack_head_is_max_ord.
Print Assumptions stack_produce_preserves_inv.
Print Assumptions stack_consume_preserves_inv.
Print Assumptions stack_lifo.
Print Assumptions stack_head_blocks.
Print Assumptions stack_reentry_at_lifo_place.
Print Assumptions stack_reissue_keeps_ord.
Print Assumptions stack_peek_restore_needs_no_settle.
Print Assumptions stack_settle_terminates.
Print Assumptions stack_plan_permutation_invariant.
Print Assumptions stack_without_settle_breaks_inv.
Print Assumptions settle_trace_covered.
Print Assumptions stack_settle_keeps_unspent_records.
