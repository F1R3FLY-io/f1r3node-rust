(* ═══════════════════════════════════════════════════════════════════════════
   HeadOnlyDisciplines.v — head-only matching of the queue discipline (P4)

   This module models the queue discipline of the native spaces (plan
   decision 11) and the plans of GenericSpace (P4 design B.4). A queue is
   head-only on both sides. The head of the records and the head of the
   waiting continuations are the entries with the least (ord, bincode
   bytes), and only the two heads interact. A head that does not match
   blocks the entries behind it. Every operation bumps the collection
   counter (M-1, decision 31). A fresh operation takes the value of the
   counter as its ord. So its element joins the tail of its side, and it
   interacts only when its side is empty. A re-issued persistent operation
   keeps the ord of its first round (decision 43). So its element keeps
   its record identity and its FIFO place.

   The produce plan (offer, decision 30):
     if a record is queued: store the datum, stop             (FIFO tail)
     loop: W := the head waiter, or store and stop
           if W does not match the datum (pattern and guard): store, stop
           stage Fire(datum, W)
           Done if the datum is spent
           re-issue if W stays (A10)
           re-issue at FIRE_ROUND_CAP fires (I-21)
           else offer to the next head
   The consume plan (receive) is symmetric: the continuation waits behind
   earlier waiters, then meets the head records one by one. After a peek
   fire of a non-persistent record, the code also stages the re-store of
   that record (decision 45, Rule E: code_ops).
   The round of a re-issued persistent element (decision 43, QD-9):
     bump the counter, keep the first-round ord             (operation_ord)
     if an older entry is the head of its side: store or park the element
       at its place, stop
     else: the loop of the produce plan or of the consume plan, from the
       element
     a re-issue keeps the ord only for a persistent element (the scope
       rule). A non-persistent element runs again as a fresh operation.

   Sources
   - Design: the P4 design report B.4 (plans, the invariant INV, the loop
     bound) and E.1 (the headlines of this module, the queue part). The
     design addendum of decision 43, sections 43.2 to 43.4 and 43.9 (the
     placement rule, the scope rule, SHADOW, L1 to L4 and T1 to T4). The
     design report of decision 45: sections C.1, C.7 and C.8 (the staged
     operations of a peek, the hot store and the queue) and E.5.
   - Plan: decisions 11 (head-only), 30 (re-offer of a surviving datum), 31
     (every operation bumps the counter) and 43 (a re-issued persistent
     operation keeps its first-round ord), and revision I-21
     (FIRE_ROUND_CAP = 64). Decision 45 (Rule E): a peek fire removes each
     non-persistent record that it binds, and the agent stores the record
     again.
   - Code: the commit of decision 45 (Rule E) in this repository, which
     keeps the plans of decision 43 and adds the re-store of a peeked
     record. The line numbers below are the line numbers of that commit.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   precedes, head_from, head
                            spaces/disciplines/queue.rs:41-59, 88-123
                            (head_metered: the least (ord, bytes), the
                            earlier index on a tie, and Best::challenge)
   reaches_head             spaces/disciplines/queue.rs:62 (queued == 0)
   fire_round_cap, after_fire
                            reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP =
                            64, FireRounds::after_fire)
   offer, offer_loop        spaces/generic.rs:1421-1488 (offer_head, the
                            head driver of Discipline::offer)
   receive, receive_loop    spaces/generic.rs:1497-1581 (receive_head, the
                            head driver of Discipline::receive. The row of
                            code_ops covers its re-store of decision 45.)
   head_waiter, head_record spaces/generic.rs:737-753, 757-770, 774-785
                            (Cx::scan_head, Cx::head_waiter, Cx::head_datum)
   bump                     spaces/counter.rs:165-245 (the ord is the
                            counter value, and the first operation takes 0
                            and opens the counter at 1)
   produce, consume         spaces/generic.rs:2172-2304 (run_with of
                            FamilySpace: bump, then plan)
   legacy_offer             the legacy waiter scan (the legacy
                            GenericRSpace find_fireable_continuation_with_
                            similarity scans every waiter, commit ff479042)
   in_flight, flight_ord    spaces/agent.rs:108, 151
                            (ProduceOp::reissued_ord and
                            ConsumeOp::reissued_ord: the first-round ord of
                            a re-issued persistent operation)
   operation_ord            spaces/generic.rs:1612-1640 (the check
                            0 <= reissued_ord < bump.ord, else
                            Inconsistent, and the bump value for a fresh
                            operation)
   ord_precedes             spaces/disciplines/queue.rs:65-74
                            (QueueDiscipline::precedes: ord < head)
   reentry_at_head          spaces/generic.rs:826-847, 789-809
                            (Cx::at_head and Cx::head_ord: an empty side,
                            or an ord that precedes the head, and
                            Inconsistent on an equal ord)
   reenter_offer, reenter_receive
                            spaces/generic.rs:1421-1488, 1497-1581
                            (offer_head and receive_head: store or park at
                            its place when not at the head, else the loop)
   reenter_produce, reenter_consume
                            spaces/generic.rs:2172-2304 (run_with of
                            FamilySpace with a reissued_ord: bump, then
                            operation_ord, then the plan)
   reissued_ord, flight_after
                            spaces/generic.rs:1480, 1573 (offer_head and
                            receive_head: the re-issue sets
                            reissued_ord: op.persistent.then_some(ord))
   round, run_rounds        reified/reducer_hooks.rs:645-695
                            (dispatch_space_outcome: a re-issue is a new
                            Intent::Space, the last dispatch branch of the
                            same deploy)
   code_staged, code_of, code_ops
                            spaces/generic.rs:976-987, 1617-1626, 1572-1577
                            and 1550-1579 (Cx::restore and Cx::restore_on
                            stage a Store of the same record bytes with
                            persist = false, and restore_peeked calls
                            Discipline::restore, whose default is
                            Cx::restore. receive_head copies the record of
                            a peek fire when the record is not persistent.
                            Each FireStep branch calls fire_receive, then
                            restore_peeked.)
   remove_key, sub_step, sub_records
                            rspace/space_play.rs:136, 196, 294, 369-386,
                            389-413 and 456-472,
                            replay_rspace/space_replay.rs:186, 288-289 and
                            430, and hot_store.rs:879-897 (a Store calls
                            store_data, which calls put_datum. put_datum
                            inserts at index 0 with no deduplication. A
                            fire always calls remove_matched_datum_and_join
                            or store_persistent_data, which remove each
                            bound stored record that is not persistent.
                            The incoming datum of a produce-side fire has
                            no index, so no stored record leaves.)
   retention_step, retention_records
                            rspace/space_play.rs:186-195, 285-293 and
                            replay_rspace/space_replay.rs:272-287, 421-429
                            (the retention before decision 45, kept as
                            comments: a peek fire kept its records)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - fits w d is the metered match of the payload of d against the pattern
     of w followed by the guard of w (SpaceMatcher::matches,
     spaces/matching.rs:107-121). The model takes it as a section variable:
     every theorem holds for every matcher.
   - The bincode bytes of an entry give a strict total order on distinct
     entries. The model writes rank for the position of those bytes in that
     order. Two entries with one (ord, rank) are byte-identical. The premise
     key_injective of the permutation theorem states this, and the corollary
     plan_permutation_invariant_instance discharges it.
   - The staged view of a collection is a record list and a waiter list. A
     fire removes the fired partner at its view index unless it stays.
   - A peek fire of a non-persistent record removes the record in the
     substrate, and the code stores the same record again right after the
     fire (decision 45, Rule E: Cx::restore). o_ops is the logical plan.
     It keeps the peeked record in o_state, and it has no re-store.
     code_ops adds the re-store after each such fire. sub_records applies
     the Rule E effects of code_ops to the records of the view. The
     re-stored record goes to the front of its list.
     queue_restore_realizes_plan proves that the result holds the records
     of o_state, up to the list order. queue_plan_permutation_invariant
     shows that the list order does not change a plan. EventCorrelation.v
     (H11) models the removal and the re-store in the substrate.
   - The ord is a nat. The code checks i64 overflow with CounterOverflow
     (spaces/counter.rs:188, 221), so every ord of the code is a nat.
   - An element in flight is a record or a waiter with the ord of its first
     round. The list F of the elements in flight belongs to one collection.
     The re-issue of a non-persistent element is a fresh operation, so F
     does not hold it.
   - The model gives None for the Inconsistent errors of operation_ord and
     at_head. queue_reissue_keeps_ord shows that wf' excludes them.
   - A run with re-entries (run_rounds) is a list of rounds. A round runs a
     fresh operation or the re-entry of an element of F. A deploy ends only
     when its re-issues complete (design 43.4, T2). So F is empty at every
     deploy boundary.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. queue_head_is_min_ord             the head has the least (ord, rank)
    2. queue_produce_preserves_inv       a produce keeps INV
    3. queue_consume_preserves_inv       a consume keeps INV
    4. queue_fifo                        records leave in arrival order
    5. queue_head_blocks                 a blocked head blocks the rest
    6. queue_loop_bounded_or_reissues    at most FIRE_ROUND_CAP fires, at
                                         most one more than the queue length
    7. queue_ord_fresh                   the ord of a fresh operation is
                                         new
    8. queue_plan_permutation_invariant  the plan ignores the list order
    9. legacy_scan_violates_head_blocks  the legacy scan breaks head-only
   Decision 43 (a re-issued persistent operation keeps its first-round
   ord):
   10. persistent_reentry_never_spent    L2: a re-entered persistent
                                         element is stored, parked or in
                                         flight again
   11. queue_reissue_keeps_ord           a re-issue keeps its first-round
                                         ord and its record, the counter
                                         grows by one, and wf' holds
   12. queue_reentry_preserves_shadow_inv
                                         every round keeps wf' and SHADOW
   13. queue_quiescent_inv               INV holds when nothing is in
                                         flight, so at every deploy
                                         boundary
   14. queue_fifo_with_reentries         non-persistent records leave in
                                         arrival order, with re-entries
   15. queue_head_blocks_reentry         an older head holds a re-entered
                                         element at its place
   16. queue_reentry_loop_bounded_or_reissues
                                         the loop bound of a re-entry, and
                                         the scope rule of a re-issue
   17. queue_reentry_permutation_invariant
                                         a re-entry ignores the list order
   18. fresh_ord_reissue_loses_first_round_place
                                         C0: with a fresh ord, the element
                                         queues behind a younger record
   19. nonpersistent_first_ord_reentry_breaks_quiescent_inv
                                         C1: a non-persistent element that
                                         keeps its ord breaks INV at a
                                         deploy boundary
   Decision 45 (Rule E: a peek fire removes the record that it binds, and
   the code stores the record again):
   20. queue_restore_realizes_plan       the Rule E effects of the code
                                         operations give the records of the
                                         plan, up to the list order

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `exact` with a let-bound lemma over `offer_loop fire_round_cap`
     unifies very slowly (minutes): the unifier unfolds the fuel 64. Use
     `pose proof`, then `cbv zeta`, then `rewrite Nat.add_0_l`.
   - `repeat split` also splits the implications of a conclusion. Build
     the conjunctions with explicit `conj`.
   - A fixpoint applied to a constant fuel does not unfold by `cbn`. State
     the helper lemmas over every fuel and start with
     `intros [| fuel]; cbn [offer_loop]`.
   - `all: try` hides failing branches. The proofs use the helper lemmas
     offer_props and receive_props instead.
   - The injection tactic and the inversion tactic fail on an equation
     with offer_loop fire_round_cap (decision 43). Each tactic normalizes
     the loop and runs out of memory. receive_loop fire_round_cap has the
     same form. Use some_injective or triple_injective, then subst.
   - A proof that unfolds SHADOW or no_pending_match (a match on the two
     heads) over o_state (offer_loop fire_round_cap ...) and rewrites in it
     passes its tactics, but its Qed does not end (decision 43). The likely
     cause: the kernel reduces the loop, and each level of the loop holds
     the rest of the loop three times (o_ops, o_state, o_reissue). State
     such a lemma over an outcome variable, with offer_end or receive_end
     as a premise (offer_end_shadow). Then instantiate the variable.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

(* ── 1. The fire rounds ────────────────────────────────────────────────── *)

Definition fire_round_cap : nat := 64.

Inductive fire_step : Type := FDone | FContinue | FDivergent | FRoundCap.

Inductive reissue : Type := ReissueDivergent | ReissueRoundCap.

(* FireRounds::after_fire with the count of fires after this one. *)
Definition after_fire (fired : nat) (spent stays : bool) : fire_step :=
  if spent then FDone
  else if stays then FDivergent
  else if fire_round_cap <=? fired then FRoundCap
  else FContinue.

(* ── 2. Entries and heads ──────────────────────────────────────────────── *)

Section Queue.
  Variable V : Type.     (* the payload of a datum *)
  Variable P : Type.     (* the pattern and guard of a continuation *)
  Variable fits : P -> V -> bool.

  Record record : Type := mkRecord { r_ord : nat; r_rank : nat; r_val : V; r_pers : bool }.
  Record waiter : Type :=
    mkWaiter { w_ord : nat; w_rank : nat; w_pat : P; w_pers : bool; w_peek : bool }.

  (* precedes: a lesser ord, or an equal ord and lesser bytes. *)
  Definition precedes_key (o1 r1 o2 r2 : nat) : bool :=
    (o1 <? o2) || ((o1 =? o2) && (r1 <? r2)).

  Section Heads.
    Variable E : Type.
    Variable ord : E -> nat.
    Variable rank : E -> nat.

    Definition precedes (a b : E) : bool := precedes_key (ord a) (rank a) (ord b) (rank b).

    (* QueueDiscipline::head: the first entry with the least key in list
       order. A later entry replaces the best only when it precedes it. *)
    Fixpoint head_from (best : option (nat * E)) (idx : nat) (l : list E) : option (nat * E) :=
      match l with
      | [] => best
      | e :: rest =>
          let best' :=
            match best with
            | None => Some (idx, e)
            | Some (bi, b) => if precedes e b then Some (idx, e) else Some (bi, b)
            end in
          head_from best' (S idx) rest
      end.

    Definition head_index (l : list E) : option (nat * E) := head_from None 0 l.

    Definition key_le (a b : E) : Prop :=
      ord a < ord b \/ (ord a = ord b /\ rank a <= rank b).

    Lemma precedes_false_le : forall a b, precedes a b = false -> key_le b a.
    Proof.
      intros a b H. unfold precedes, precedes_key in H.
      apply orb_false_iff in H as [H1 H2]. apply Nat.ltb_ge in H1.
      unfold key_le.
      destruct (Nat.eq_dec (ord a) (ord b)) as [E1 | E1].
      - right. split; [symmetry; exact E1 |].
        rewrite (proj2 (Nat.eqb_eq _ _) E1) in H2. simpl in H2. apply Nat.ltb_ge in H2. exact H2.
      - left. lia.
    Qed.

    Lemma precedes_true_lt : forall a b, precedes a b = true -> key_le a b /\ ~ key_le b a.
    Proof.
      intros a b H. unfold precedes, precedes_key in H. unfold key_le.
      apply orb_true_iff in H as [H | H].
      - apply Nat.ltb_lt in H. split; [left; exact H | lia].
      - apply andb_true_iff in H as [H1 H2]. apply Nat.eqb_eq in H1. apply Nat.ltb_lt in H2.
        split; [right; split; [exact H1 | lia] | lia].
    Qed.

    Lemma key_le_trans : forall a b c, key_le a b -> key_le b c -> key_le a c.
    Proof. unfold key_le. intros a b c H1 H2. lia. Qed.

    Lemma key_le_total : forall a b, key_le a b \/ key_le b a.
    Proof. intros a b. unfold key_le. lia. Qed.

    Lemma key_le_refl : forall a, key_le a a.
    Proof. intros a. right. split; [reflexivity | lia]. Qed.

    (* The invariant of head_from: the best is an entry of the prefix with
       the least key, and its index is its position. *)
    Lemma head_from_spec :
      forall l pre best idx,
        idx = length pre ->
        (match best with
         | None => pre = []
         | Some (bi, b) => nth_error pre bi = Some b /\ forall x, In x pre -> key_le b x
         end) ->
        match head_from best idx l with
        | None => pre ++ l = []
        | Some (bi, b) =>
            nth_error (pre ++ l) bi = Some b /\ forall x, In x (pre ++ l) -> key_le b x
        end.
    Proof.
      induction l as [| e rest IH]; intros pre best idx Hidx Hbest.
      - simpl. rewrite app_nil_r. destruct best as [[bi b] |]; [exact Hbest | exact Hbest].
      - simpl. replace (pre ++ e :: rest) with ((pre ++ [e]) ++ rest) by (rewrite <- app_assoc; reflexivity).
        apply IH; [rewrite length_app; simpl; lia |].
        destruct best as [[bi b] |].
        + destruct Hbest as [Hnth Hmin]. destruct (precedes e b) eqn:Ep.
          * split.
            -- rewrite nth_error_app2 by lia. rewrite Hidx, Nat.sub_diag. reflexivity.
            -- intros x Hx. apply in_app_or in Hx as [Hx | [Hx | []]].
               ++ apply precedes_true_lt in Ep as [Hle _].
                  exact (key_le_trans _ _ _ Hle (Hmin x Hx)).
               ++ subst. apply key_le_refl.
          * split.
            -- rewrite nth_error_app1; [exact Hnth |]. apply nth_error_Some. rewrite Hnth. discriminate.
            -- intros x Hx. apply in_app_or in Hx as [Hx | [Hx | []]].
               ++ exact (Hmin x Hx).
               ++ subst. apply precedes_false_le. exact Ep.
        + subst pre. simpl in Hidx. subst idx. split; [reflexivity |].
          intros x [Hx | []]. subst. apply key_le_refl.
    Qed.

    Lemma head_index_spec :
      forall l,
        match head_index l with
        | None => l = []
        | Some (bi, b) => nth_error l bi = Some b /\ forall x, In x l -> key_le b x
        end.
    Proof.
      intros l. pose proof (head_from_spec l [] None 0 eq_refl eq_refl) as H. exact H.
    Qed.

    Lemma head_index_none : forall l, head_index l = None <-> l = [].
    Proof.
      intros l. split.
      - intros H. pose proof (head_index_spec l) as Hs. rewrite H in Hs. exact Hs.
      - intros ->. reflexivity.
    Qed.

    Definition head (l : list E) : option E := option_map snd (head_index l).
  End Heads.

  Arguments head_index {E} ord rank l.
  Arguments head {E} ord rank l.
  Arguments key_le {E} ord rank a b.

  Definition record_head (l : list record) : option record := head r_ord r_rank l.
  Definition waiter_head (l : list waiter) : option waiter := head w_ord w_rank l.

  (* ── 3. States, operations and plans ───────────────────────────────── *)

  (* A collection: its records, its waiting continuations, and its counter
     (None before the first operation). *)
  Record qstate : Type := mkQ { q_records : list record; q_waiters : list waiter; q_counter : option nat }.

  (* The bump of spaces/counter.rs: the ord is the counter value, or 0 for
     the first operation, which opens the counter at 1. *)
  Definition bump_ord (c : option nat) : nat := match c with None => 0 | Some v => v end.
  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).

  (* A staged operation of a plan, after the bump. *)
  Inductive staged : Type :=
  | SStore : record -> staged
  | SPark : waiter -> staged
  | SFireProduce : waiter -> record -> staged
  | SFireConsume : waiter -> record -> staged.

  Record outcome : Type := mkOutcome {
    o_ops : list staged;
    o_state : qstate;
    o_reissue : option reissue
  }.

  Fixpoint remove_at {A : Type} (i : nat) (l : list A) : list A :=
    match l, i with
    | [], _ => []
    | _ :: rest, 0 => rest
    | x :: rest, S j => x :: remove_at j rest
    end.

  Definition store_record (st : qstate) (r : record) : qstate :=
    mkQ (r :: q_records st) (q_waiters st) (q_counter st).

  Definition park_waiter (st : qstate) (w : waiter) : qstate :=
    mkQ (q_records st) (w :: q_waiters st) (q_counter st).

  (* The loop of offer. The datum r has its ord, and the queue has no
     record. *)
  Fixpoint offer_loop (fuel fired : nat) (st : qstate) (r : record) : outcome :=
    match head_index w_ord w_rank (q_waiters st) with
    | None => mkOutcome [SStore r] (store_record st r) None
    | Some (wi, w) =>
        if negb (fits (w_pat w) (r_val r)) then mkOutcome [SStore r] (store_record st r) None
        else
          let spent := negb (r_pers r) && negb (w_peek w) in
          let stays := w_pers w in
          let st1 := if stays then st else mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st) in
          match after_fire (S fired) spent stays with
          | FDone => mkOutcome [SFireProduce w r] st1 None
          | FDivergent => mkOutcome [SFireProduce w r] st1 (Some ReissueDivergent)
          | FRoundCap => mkOutcome [SFireProduce w r] st1 (Some ReissueRoundCap)
          | FContinue =>
              match fuel with
              | 0 => mkOutcome [SFireProduce w r] st1 (Some ReissueRoundCap)
              | S fuel' =>
                  let rest := offer_loop fuel' (S fired) st1 r in
                  mkOutcome (SFireProduce w r :: o_ops rest) (o_state rest) (o_reissue rest)
              end
          end
    end.

  (* offer_head: a datum joins the tail when a record is queued. *)
  Definition offer (st : qstate) (r : record) : outcome :=
    match q_records st with
    | _ :: _ => mkOutcome [SStore r] (store_record st r) None
    | [] => offer_loop fire_round_cap 0 st r
    end.

  (* The loop of receive. The continuation k has its ord, and no waiter
     is queued. *)
  Fixpoint receive_loop (fuel fired : nat) (st : qstate) (k : waiter) : outcome :=
    match head_index r_ord r_rank (q_records st) with
    | None => mkOutcome [SPark k] (park_waiter st k) None
    | Some (hi, h) =>
        if negb (fits (w_pat k) (r_val h)) then mkOutcome [SPark k] (park_waiter st k) None
        else
          let spent := negb (w_pers k) in
          let stays := r_pers h || w_peek k in
          let st1 := if stays then st else mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st) in
          match after_fire (S fired) spent stays with
          | FDone => mkOutcome [SFireConsume k h] st1 None
          | FDivergent => mkOutcome [SFireConsume k h] st1 (Some ReissueDivergent)
          | FRoundCap => mkOutcome [SFireConsume k h] st1 (Some ReissueRoundCap)
          | FContinue =>
              match fuel with
              | 0 => mkOutcome [SFireConsume k h] st1 (Some ReissueRoundCap)
              | S fuel' =>
                  let rest := receive_loop fuel' (S fired) st1 k in
                  mkOutcome (SFireConsume k h :: o_ops rest) (o_state rest) (o_reissue rest)
              end
          end
    end.

  (* receive_head: a continuation joins the tail when a waiter is
     queued. *)
  Definition receive (st : qstate) (k : waiter) : outcome :=
    match q_waiters st with
    | _ :: _ => mkOutcome [SPark k] (park_waiter st k) None
    | [] => receive_loop fire_round_cap 0 st k
    end.

  (* run_with: bump the counter, then plan with the ord of the bump. rank is
     the byte rank of the new entry. *)
  Definition bumped (st : qstate) : qstate := mkQ (q_records st) (q_waiters st) (bump_next (q_counter st)).

  Definition produce (st : qstate) (rank : nat) (v : V) (pers : bool) : outcome :=
    offer (bumped st) (mkRecord (bump_ord (q_counter st)) rank v pers).

  Definition consume (st : qstate) (rank : nat) (pat : P) (pers peek : bool) : outcome :=
    receive (bumped st) (mkWaiter (bump_ord (q_counter st)) rank pat pers peek).

  (* INV (P4 design B.4): no pending match between the two heads. *)
  Definition no_pending_match (st : qstate) : Prop :=
    match record_head (q_records st), waiter_head (q_waiters st) with
    | Some r, Some w => fits (w_pat w) (r_val r) = false
    | _, _ => True
    end.

  (* ── 4. Headline 1: the head ───────────────────────────────────────── *)

  (* Headline 1. The head of each side is an entry of that side with the
     least (ord, rank), and a side has a head exactly when it is not
     empty. *)
  Theorem queue_head_is_min_ord :
    (forall l r, record_head l = Some r ->
       In r l /\ forall x, In x l -> key_le r_ord r_rank r x) /\
    (forall l w, waiter_head l = Some w ->
       In w l /\ forall x, In x l -> key_le w_ord w_rank w x) /\
    (forall l, record_head l = None <-> l = []) /\
    (forall l, waiter_head l = None <-> l = []).
  Proof.
    split; [| split; [| split]].
    - intros l r H. unfold record_head, head in H.
      pose proof (head_index_spec record r_ord r_rank l) as Hs.
      destruct (head_index r_ord r_rank l) as [[bi b] |]; [| discriminate].
      injection H as <-. destruct Hs as [Hnth Hmin].
      split; [apply nth_error_In with bi; exact Hnth | exact Hmin].
    - intros l w H. unfold waiter_head, head in H.
      pose proof (head_index_spec waiter w_ord w_rank l) as Hs.
      destruct (head_index w_ord w_rank l) as [[bi b] |]; [| discriminate].
      injection H as <-. destruct Hs as [Hnth Hmin].
      split; [apply nth_error_In with bi; exact Hnth | exact Hmin].
    - intros l. unfold record_head, head. rewrite <- (head_index_none record r_ord r_rank l).
      destruct (head_index r_ord r_rank l); split; intro H; try discriminate; reflexivity.
    - intros l. unfold waiter_head, head. rewrite <- (head_index_none waiter w_ord w_rank l).
      destruct (head_index w_ord w_rank l); split; intro H; try discriminate; reflexivity.
  Qed.

  (* ── 5. Well-formed states ───────────────────────────────────────── *)

  (* A well-formed collection: the ords of each side are distinct, and every
     ord is below the counter value (the next ord). *)
  Definition wf (st : qstate) : Prop :=
    NoDup (map r_ord (q_records st)) /\ NoDup (map w_ord (q_waiters st)) /\
    (forall r, In r (q_records st) -> r_ord r < bump_ord (q_counter st)) /\
    (forall w, In w (q_waiters st) -> w_ord w < bump_ord (q_counter st)).

  Definition keys_unique {E : Type} (ord rank : E -> nat) (l : list E) : Prop :=
    forall x y, In x l -> In y l -> ord x = ord y -> rank x = rank y -> x = y.

  Lemma NoDup_map_inj_in :
    forall (A B : Type) (f : A -> B) (l : list A) x y,
      NoDup (map f l) -> In x l -> In y l -> f x = f y -> x = y.
  Proof.
    intros A B f l. induction l as [| a l IH]; intros x y Hnd Hx Hy Hf; [destruct Hx |].
    simpl in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct Hx as [-> | Hx]; destruct Hy as [-> | Hy].
    - reflexivity.
    - exfalso. apply Hnot. rewrite Hf. apply in_map. exact Hy.
    - exfalso. apply Hnot. rewrite <- Hf. apply in_map. exact Hx.
    - apply IH; assumption.
  Qed.

  Lemma nodup_keys_unique :
    forall (E : Type) (ord rank : E -> nat) l, NoDup (map ord l) -> keys_unique ord rank l.
  Proof.
    intros E ord rank l Hnd x y Hx Hy Ho _. exact (NoDup_map_inj_in _ _ ord l x y Hnd Hx Hy Ho).
  Qed.

  Lemma head_spec :
    forall (E : Type) (ord rank : E -> nat) l h,
      head ord rank l = Some h -> In h l /\ forall x, In x l -> key_le ord rank h x.
  Proof.
    intros E ord rank l h H. unfold head in H.
    pose proof (head_index_spec E ord rank l) as Hs.
    destruct (head_index ord rank l) as [[bi b] |]; [| discriminate].
    injection H as <-. destruct Hs as [Hnth Hmin].
    split; [apply nth_error_In with bi; exact Hnth | exact Hmin].
  Qed.

  Lemma head_exists :
    forall (E : Type) (ord rank : E -> nat) l, l <> [] -> exists h, head ord rank l = Some h.
  Proof.
    intros E ord rank l Hne. unfold head.
    destruct (head_index ord rank l) as [[bi b] |] eqn:Eh.
    - exists b. reflexivity.
    - apply head_index_none in Eh. contradiction.
  Qed.

  (* With unique keys, the head is the least entry. *)
  Lemma head_unique :
    forall (E : Type) (ord rank : E -> nat) l b,
      keys_unique ord rank l -> In b l -> (forall x, In x l -> key_le ord rank b x) ->
      head ord rank l = Some b.
  Proof.
    intros E ord rank l b Hk Hb Hmin.
    assert (Hne : l <> []) by (intros ->; destruct Hb).
    destruct (head_exists E ord rank l Hne) as [h Hh].
    destruct (head_spec E ord rank l h Hh) as [Hin Hhmin].
    rewrite Hh. f_equal.
    specialize (Hmin h Hin). specialize (Hhmin b Hb). unfold key_le in Hmin, Hhmin.
    apply Hk; [exact Hin | exact Hb | lia | lia].
  Qed.

  (* A fresh entry at the front of a non-empty side does not change the
     head of that side. *)
  Lemma head_cons_fresh :
    forall (E : Type) (ord rank : E -> nat) e l,
      keys_unique ord rank l -> l <> [] -> (forall x, In x l -> ord x < ord e) ->
      head ord rank (e :: l) = head ord rank l.
  Proof.
    intros E ord rank e l Hk Hne Hlt.
    destruct (head_exists E ord rank l Hne) as [h Hh].
    destruct (head_spec E ord rank l h Hh) as [Hin Hmin]. rewrite Hh.
    destruct (head_exists E ord rank (e :: l) ltac:(discriminate)) as [h' Hh'].
    destruct (head_spec E ord rank (e :: l) h' Hh') as [Hin' Hmin']. rewrite Hh'.
    destruct Hin' as [<- | Hin'].
    - specialize (Hmin' h (or_intror Hin)). specialize (Hlt h Hin). unfold key_le in Hmin'. lia.
    - f_equal. specialize (Hmin' h (or_intror Hin)). specialize (Hmin h' Hin').
      unfold key_le in Hmin, Hmin'. apply Hk; [exact Hin' | exact Hin | lia | lia].
  Qed.

  Lemma remove_at_in : forall (A : Type) i (l : list A) x, In x (remove_at i l) -> In x l.
  Proof.
    intros A i l. revert i. induction l as [| a l IH]; intros [| i] x H; simpl in H.
    - destruct H.
    - destruct H.
    - right. exact H.
    - destruct H as [-> | H]; [left; reflexivity | right; exact (IH i x H)].
  Qed.

  Lemma remove_at_perm :
    forall (A : Type) i (l : list A) x, nth_error l i = Some x -> Permutation l (x :: remove_at i l).
  Proof.
    intros A i l. revert i. induction l as [| a l IH]; intros [| i] x H; simpl in H; try discriminate.
    - injection H as <-. reflexivity.
    - simpl. eapply perm_trans; [apply perm_skip; exact (IH i x H) | apply perm_swap].
  Qed.

  Lemma remove_at_nodup :
    forall (A B : Type) (f : A -> B) i l, NoDup (map f l) -> NoDup (map f (remove_at i l)).
  Proof.
    intros A B f i l. revert i. induction l as [| a l IH]; intros [| i] Hnd; simpl in *;
      try exact Hnd.
    - apply NoDup_cons_iff in Hnd as [_ Hnd]. exact Hnd.
    - apply NoDup_cons_iff in Hnd as [Hnot Hnd]. apply NoDup_cons_iff. split; [| apply IH; exact Hnd].
      intros Hin. apply Hnot. apply in_map_iff in Hin as [y [Hy Hin]].
      apply in_map_iff. exists y. split; [exact Hy | exact (remove_at_in A i l y Hin)].
  Qed.

  Lemma head_index_head :
    forall (E : Type) (ord rank : E -> nat) l i e,
      head_index ord rank l = Some (i, e) -> head ord rank l = Some e /\ nth_error l i = Some e.
  Proof.
    intros E ord rank l i e H. unfold head. rewrite H. split; [reflexivity |].
    pose proof (head_index_spec E ord rank l) as Hs. rewrite H in Hs. exact (proj1 Hs).
  Qed.

  (* ── 6. Headlines 2 and 3: INV ────────────────────────────────────── *)

  (* The state facts that an offer keeps (the datum r has no queued
     record ahead of it). *)
  Definition offer_props (st : qstate) (r : record) (st' : qstate) : Prop :=
    no_pending_match st' /\ (q_records st' = [] \/ q_records st' = [r]) /\
    q_counter st' = q_counter st /\ (forall w, In w (q_waiters st') -> In w (q_waiters st)).

  Lemma record_head_single : forall r, record_head [r] = Some r.
  Proof. reflexivity. Qed.

  Lemma offer_store_props :
    forall st r,
      q_records st = [] ->
      (q_waiters st = [] \/
       exists w, waiter_head (q_waiters st) = Some w /\ fits (w_pat w) (r_val r) = false) ->
      offer_props st r (store_record st r).
  Proof.
    intros st r Hrec Hw. unfold offer_props, store_record, no_pending_match.
    cbn [q_records q_waiters q_counter]. rewrite Hrec, record_head_single.
    split; [| split; [right; reflexivity | split; [reflexivity | intros w0 H0; exact H0]]].
    destruct Hw as [-> | [w [Hh Hf]]]; [exact I |]. rewrite Hh. exact Hf.
  Qed.

  Lemma offer_fired_props :
    forall st r wi (w : waiter),
      q_records st = [] ->
      offer_props st r
        (if w_pers w then st else mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st)).
  Proof.
    intros st r wi w Hrec. unfold offer_props, no_pending_match.
    destruct (w_pers w); cbn [q_records q_waiters q_counter]; rewrite Hrec.
    - split; [exact I | split; [left; reflexivity | split; [reflexivity | intros w0 H0; exact H0]]].
    - split; [exact I | split; [left; reflexivity | split; [reflexivity | apply remove_at_in]]].
  Qed.

  Lemma offer_props_trans :
    forall st st1 r X,
      q_counter st1 = q_counter st ->
      (forall w, In w (q_waiters st1) -> In w (q_waiters st)) ->
      offer_props st1 r X -> offer_props st r X.
  Proof.
    intros st st1 r X Hc Hw [H1 [H2 [H3 H4]]].
    split; [exact H1 | split; [exact H2 | split; [rewrite H3; exact Hc |]]].
    intros w0 H0. apply Hw. apply H4. exact H0.
  Qed.

  Lemma offer_loop_state :
    forall fuel fired st r,
      q_records st = [] -> offer_props st r (o_state (offer_loop fuel fired st r)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hrec;
      unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |] eqn:Eh.
    all: try (cbn [o_state]; apply offer_store_props; [exact Hrec |];
              left; apply (head_index_none waiter w_ord w_rank); exact Eh).
    all: destruct (head_index_head waiter w_ord w_rank _ _ _ Eh) as [Hhead _].
    all: destruct (fits (w_pat w) (r_val r)) eqn:Efit; cbn [negb].
    all: try (cbn [o_state]; apply offer_store_props; [exact Hrec |];
              right; exists w; split; [exact Hhead | exact Efit]).
    all: destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [o_state]; try (apply offer_fired_props; exact Hrec).
    pose proof (offer_fired_props st r wi w Hrec) as [_ [_ [Hc Hw]]].
    apply (offer_props_trans st _ r _ Hc Hw).
    apply IH. destruct (w_pers w); exact Hrec.
  Qed.

  (* The symmetric facts of a receive (the continuation k has no queued
     waiter ahead of it). The removed records are the heads, in order. *)
  Definition receive_props (st : qstate) (k : waiter) (st' : qstate) : Prop :=
    no_pending_match st' /\ (q_waiters st' = [] \/ q_waiters st' = [k]) /\
    q_counter st' = q_counter st /\ (forall r, In r (q_records st') -> In r (q_records st)).

  Lemma waiter_head_single : forall w, waiter_head [w] = Some w.
  Proof. reflexivity. Qed.

  Lemma receive_park_props :
    forall st k,
      q_waiters st = [] ->
      (q_records st = [] \/
       exists r, record_head (q_records st) = Some r /\ fits (w_pat k) (r_val r) = false) ->
      receive_props st k (park_waiter st k).
  Proof.
    intros st k Hw Hr. unfold receive_props, park_waiter, no_pending_match.
    cbn [q_records q_waiters q_counter]. rewrite Hw, waiter_head_single.
    split; [| split; [right; reflexivity | split; [reflexivity | intros r0 H0; exact H0]]].
    destruct Hr as [-> | [r [Hh Hf]]]; [exact I |]. rewrite Hh. exact Hf.
  Qed.

  Lemma receive_fired_props :
    forall st k hi (h : record),
      q_waiters st = [] ->
      receive_props st k
        (if r_pers h || w_peek k then st
         else mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st)).
  Proof.
    intros st k hi h Hw. unfold receive_props, no_pending_match.
    destruct (r_pers h || w_peek k); cbn [q_records q_waiters q_counter]; rewrite Hw;
      (split; [destruct (record_head _); exact I |]).
    - split; [left; reflexivity | split; [reflexivity | intros r0 H0; exact H0]].
    - split; [left; reflexivity | split; [reflexivity | apply remove_at_in]].
  Qed.

  Lemma receive_props_trans :
    forall st st1 k X,
      q_counter st1 = q_counter st ->
      (forall r, In r (q_records st1) -> In r (q_records st)) ->
      receive_props st1 k X -> receive_props st k X.
  Proof.
    intros st st1 k X Hc Hr [H1 [H2 [H3 H4]]].
    split; [exact H1 | split; [exact H2 | split; [rewrite H3; exact Hc |]]].
    intros r0 H0. apply Hr. apply H4. exact H0.
  Qed.

  Lemma receive_loop_state :
    forall fuel fired st k,
      q_waiters st = [] -> receive_props st k (o_state (receive_loop fuel fired st k)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hw;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    all: try (cbn [o_state]; apply receive_park_props; [exact Hw |];
              left; apply (head_index_none record r_ord r_rank); exact Eh).
    all: destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [Hhead _].
    all: destruct (fits (w_pat k) (r_val h)) eqn:Efit; cbn [negb].
    all: try (cbn [o_state]; apply receive_park_props; [exact Hw |];
              right; exists h; split; [exact Hhead | exact Efit]).
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
      cbn [o_state]; try (apply receive_fired_props; exact Hw).
    pose proof (receive_fired_props st k hi h Hw) as [_ [_ [Hc Hr]]].
    apply (receive_props_trans st _ k _ Hc Hr).
    apply IH. destruct (r_pers h || w_peek k); exact Hw.
  Qed.
  (* ── 7. Removal facts ─────────────────────────────────────────────── *)

  Lemma after_fire_continue :
    forall n spent stays, after_fire n spent stays = FContinue -> spent = false /\ stays = false.
  Proof.
    intros n [] [] H; unfold after_fire in H; try discriminate; split; reflexivity.
  Qed.

  Lemma remove_at_nodup_other :
    forall (E : Type) (f : E -> nat) i l h y,
      nth_error l i = Some h -> NoDup (map f l) -> In y (remove_at i l) -> f y <> f h.
  Proof.
    intros E f i l h y Hnth Hnd Hy Heq.
    pose proof (Permutation_map f (remove_at_perm E i l h Hnth)) as Hp.
    pose proof (Permutation_NoDup Hp Hnd) as Hnd'. simpl in Hnd'.
    apply NoDup_cons_iff in Hnd' as [Hnot _]. apply Hnot. rewrite <- Heq. apply in_map. exact Hy.
  Qed.

  (* The records that a consume fire removes: the record of a fire that
     neither peeks nor fires a persistent record. *)
  Definition removed_of (o : outcome) : list record :=
    flat_map (fun s => match s with
                       | SFireConsume k h => if r_pers h || w_peek k then [] else [h]
                       | _ => []
                       end) (o_ops o).

  Lemma offer_loop_removed : forall fuel fired st r, removed_of (offer_loop fuel fired st r) = [].
  Proof.
    induction fuel as [| fuel IH]; intros fired st r; unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |]; try reflexivity;
      destruct (negb (fits (w_pat w) (r_val r))); try reflexivity;
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)); try reflexivity.
    unfold removed_of in *. cbn [o_ops flat_map app]. apply IH.
  Qed.

  Lemma receive_loop_removed :
    forall fuel fired st k,
      q_waiters st = [] -> NoDup (map r_ord (q_records st)) ->
      let o := receive_loop fuel fired st k in
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall x, In x (removed_of o) -> In x (q_records st)) /\
      (forall x y, In x (removed_of o) -> In y (q_records (o_state o)) -> r_ord x < r_ord y) /\
      NoDup (map r_ord (q_records (o_state o))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hw Hnd; cbv zeta;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    all: try (unfold removed_of; cbn [o_ops o_state flat_map park_waiter q_records app];
              split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]).
    all: destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [Hhead Hnth].
    all: destruct (head_spec record r_ord r_rank _ _ Hhead) as [Hin Hmin].
    all: destruct (fits (w_pat k) (r_val h)) eqn:Efit; cbn [negb].
    all: try (unfold removed_of; cbn [o_ops o_state flat_map park_waiter q_records app];
              split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]).
    (* The removed head is older than every record that stays. *)
    all: assert (Hlt : forall y, In y (remove_at hi (q_records st)) -> r_ord h < r_ord y)
           by (intros y Hy; pose proof (Hmin y (remove_at_in record hi _ y Hy)) as Hle;
               pose proof (remove_at_nodup_other record r_ord hi _ h y Hnth Hnd Hy) as Hne;
               unfold key_le in Hle; lia).
    all: assert (Hnd1 : NoDup (map r_ord (remove_at hi (q_records st))))
           by (apply remove_at_nodup; exact Hnd).
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)) eqn:Eaf.
    all: try (destruct (r_pers h || w_peek k) eqn:Estays;
              unfold removed_of; cbn [o_ops o_state flat_map q_records app]; rewrite ?Estays;
              cbn [app];
              [ split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]
              | split; [repeat constructor |];
                split; [intros x [-> | []]; exact Hin |];
                split; [intros x y [-> | []] Hy; exact (Hlt y Hy) | exact Hnd1] ]; fail).
    (* The continuation case: the record leaves, and the loop goes on. *)
    all: pose proof (after_fire_continue _ _ _ Eaf) as [_ Hstays].
    all: unfold removed_of; cbn [o_ops o_state flat_map q_records app]; rewrite Hstays; cbn [app].
    set (st1 := mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st)).
    destruct (IH (S fired) st1 k Hw Hnd1) as [Hs [Hfrom [Hold Hnd']]].
    fold (removed_of (receive_loop fuel (S fired) st1 k)).
    pose proof (receive_loop_state fuel (S fired) st1 k Hw) as [_ [_ [_ Hsub]]].
    split; [constructor; [exact Hs |] | split; [| split; [| exact Hnd']]].
    - apply Forall_forall. intros x Hx. apply Hlt. exact (Hfrom x Hx).
    - intros x [-> | Hx]; [exact Hin | exact (remove_at_in record hi _ x (Hfrom x Hx))].
    - intros x y [-> | Hx] Hy; [apply Hlt; exact (Hsub y Hy) | exact (Hold x y Hx Hy)].
  Qed.
  Lemma offer_loop_waiters_nodup :
    forall fuel fired st r,
      NoDup (map w_ord (q_waiters st)) ->
      NoDup (map w_ord (q_waiters (o_state (offer_loop fuel fired st r)))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hnd; unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |]; try exact Hnd;
      destruct (negb (fits (w_pat w) (r_val r))); try exact Hnd;
      (assert (Hnd1 : NoDup (map w_ord (q_waiters
                 (if w_pers w then st
                  else mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st)))))
         by (destruct (w_pers w); [exact Hnd | apply remove_at_nodup; exact Hnd]));
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      cbn [o_state]; try exact Hnd1.
    apply IH. exact Hnd1.
  Qed.

  Lemma receive_loop_waiters :
    forall fuel fired st k,
      q_waiters st = [] ->
      q_waiters (o_state (receive_loop fuel fired st k)) = [] \/
      q_waiters (o_state (receive_loop fuel fired st k)) = [k].
  Proof.
    intros fuel fired st k Hw. pose proof (receive_loop_state fuel fired st k Hw) as [_ [H _]]. exact H.
  Qed.

  Lemma bump_ord_next : forall c, bump_ord (bump_next c) = S (bump_ord c).
  Proof. intros []; reflexivity. Qed.

  Lemma nodup_cons_fresh :
    forall (E : Type) (f : E -> nat) (l : list E) e,
      NoDup (map f l) -> (forall x, In x l -> f x < f e) -> NoDup (map f (e :: l)).
  Proof.
    intros E f l e Hnd Hlt. simpl. apply NoDup_cons_iff. split; [| exact Hnd].
    intros Hin. apply in_map_iff in Hin as [x [Hx Hin]]. specialize (Hlt x Hin). lia.
  Qed.

  (* Headline 2. A produce keeps INV: after it, the head waiter does not
     match the head record. *)
  Theorem queue_produce_preserves_inv :
    forall st rank v pers,
      wf st -> no_pending_match st -> no_pending_match (o_state (produce st rank v pers)).
  Proof.
    intros st rank v pers [Hndr [_ [Hbr _]]] Hinv. unfold produce, offer, bumped.
    cbn [q_records]. destruct (q_records st) as [| x xs] eqn:Erec.
    - pose proof (offer_loop_state fire_round_cap 0
                    (mkQ [] (q_waiters st) (bump_next (q_counter st)))
                    (mkRecord (bump_ord (q_counter st)) rank v pers) eq_refl) as [H _].
      exact H.
    - cbn [o_state]. unfold store_record, no_pending_match in *.
      cbn [q_records q_waiters q_counter]. rewrite Erec in Hinv.
      unfold record_head. rewrite head_cons_fresh.
      + exact Hinv.
      + apply nodup_keys_unique. first [exact Hndr | rewrite Erec in Hndr; exact Hndr].
      + discriminate.
      + intros y Hy. cbn [r_ord]. apply Hbr. first [exact Hy | rewrite Erec; exact Hy].
  Qed.

  (* Headline 3. A consume keeps INV. *)
  Theorem queue_consume_preserves_inv :
    forall st rank pat pers peek,
      wf st -> no_pending_match st -> no_pending_match (o_state (consume st rank pat pers peek)).
  Proof.
    intros st rank pat pers peek [_ [Hndw [_ Hbw]]] Hinv. unfold consume, receive, bumped.
    cbn [q_waiters]. destruct (q_waiters st) as [| x xs] eqn:Ew.
    - pose proof (receive_loop_state fire_round_cap 0
                    (mkQ (q_records st) [] (bump_next (q_counter st)))
                    (mkWaiter (bump_ord (q_counter st)) rank pat pers peek) eq_refl) as [H _].
      exact H.
    - cbn [o_state]. unfold park_waiter, no_pending_match in *.
      cbn [q_records q_waiters q_counter]. rewrite Ew in Hinv.
      unfold waiter_head. rewrite head_cons_fresh.
      + exact Hinv.
      + apply nodup_keys_unique. first [exact Hndw | rewrite Ew in Hndw; exact Hndw].
      + discriminate.
      + intros y Hy. cbn [w_ord]. apply Hbw. first [exact Hy | rewrite Ew; exact Hy].
  Qed.

  (* ── 8. Headline 7: fresh ords and well-formedness ─────────────────── *)

  Lemma produce_wf :
    forall st rank v pers, wf st -> wf (o_state (produce st rank v pers)) /\
      q_counter (o_state (produce st rank v pers)) = bump_next (q_counter st) /\
      (forall y, In y (q_records (o_state (produce st rank v pers))) ->
         In y (q_records st) \/ y = mkRecord (bump_ord (q_counter st)) rank v pers) /\
      removed_of (produce st rank v pers) = [].
  Proof.
    intros st rank v pers [Hndr [Hndw [Hbr Hbw]]]. unfold produce, offer, bumped.
    cbn [q_records]. set (r := mkRecord (bump_ord (q_counter st)) rank v pers).
    destruct (q_records st) as [| x xs] eqn:Erec.
    - set (st0 := mkQ [] (q_waiters st) (bump_next (q_counter st))).
      pose proof (offer_loop_state fire_round_cap 0 st0 r eq_refl) as [_ [Hr [Hc Hw]]].
      pose proof (offer_loop_waiters_nodup fire_round_cap 0 st0 r Hndw) as Hndw'.
      split; [| split; [exact Hc | split; [| apply offer_loop_removed]]].
      + split; [| split; [exact Hndw' | split]].
        * destruct Hr as [-> | ->]; [constructor | repeat constructor; intros []].
        * intros y Hy. rewrite Hc. cbn [st0 q_counter]. rewrite bump_ord_next.
          destruct Hr as [Hr | Hr]; rewrite Hr in Hy; [destruct Hy |].
          destruct Hy as [<- | []]. cbn [r r_ord]. lia.
        * intros w Hw0. rewrite Hc. cbn [st0 q_counter]. rewrite bump_ord_next.
          specialize (Hbw w (Hw w Hw0)). lia.
      + intros y Hy. destruct Hr as [Hr | Hr]; rewrite Hr in Hy; [destruct Hy |].
        destruct Hy as [<- | []]. right. reflexivity.
    - cbn [o_state]. unfold store_record, wf. cbn [q_records q_waiters q_counter].
      split; [| split; [reflexivity | split; [| reflexivity]]].
      + split; [| split; [exact Hndw | split]].
        * apply nodup_cons_fresh; [first [exact Hndr | rewrite Erec; exact Hndr] |].
          intros y Hy. cbn [r r_ord]. apply Hbr. first [exact Hy | rewrite Erec; exact Hy].
        * intros y [<- | Hy]; rewrite bump_ord_next; [cbn [r r_ord]; lia |].
          assert (H := Hbr y). first [specialize (H Hy) | rewrite Erec in H; specialize (H Hy)]. lia.
        * intros w Hw0. rewrite bump_ord_next. specialize (Hbw w Hw0). lia.
      + intros y [<- | Hy]; [right; reflexivity | left; first [exact Hy | rewrite Erec; exact Hy]].
  Qed.

  Lemma consume_wf :
    forall st rank pat pers peek, wf st ->
      let o := consume st rank pat pers peek in
      wf (o_state o) /\ q_counter (o_state o) = bump_next (q_counter st) /\
      (forall y, In y (q_records (o_state o)) -> In y (q_records st)) /\
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall x, In x (removed_of o) -> In x (q_records st)) /\
      (forall x y, In x (removed_of o) -> In y (q_records (o_state o)) -> r_ord x < r_ord y).
  Proof.
    intros st rank pat pers peek [Hndr [Hndw [Hbr Hbw]]]. cbv zeta.
    unfold consume, receive, bumped. cbn [q_waiters].
    set (k := mkWaiter (bump_ord (q_counter st)) rank pat pers peek).
    destruct (q_waiters st) as [| x xs] eqn:Ew.
    - set (st0 := mkQ (q_records st) [] (bump_next (q_counter st))).
      pose proof (receive_loop_state fire_round_cap 0 st0 k eq_refl) as [_ [Hw [Hc Hr]]].
      destruct (receive_loop_removed fire_round_cap 0 st0 k eq_refl Hndr)
        as [Hs [Hfrom [Hold Hndr']]].
      split; [| split; [exact Hc | split; [exact Hr | split; [exact Hs | split; [exact Hfrom | exact Hold]]]]].
      split; [exact Hndr' | split; [| split]].
      + destruct Hw as [-> | ->]; [constructor | repeat constructor; intros []].
      + intros y Hy. rewrite Hc. cbn [st0 q_counter]. rewrite bump_ord_next.
        specialize (Hbr y (Hr y Hy)). lia.
      + intros w Hw0. rewrite Hc. cbn [st0 q_counter]. rewrite bump_ord_next.
        destruct Hw as [Hw | Hw]; rewrite Hw in Hw0; [destruct Hw0 |].
        destruct Hw0 as [<- | []]. cbn [k w_ord]. lia.
    - unfold removed_of. cbn [o_state o_ops flat_map app]. unfold park_waiter, wf.
      cbn [q_records q_waiters q_counter].
      split; [| split; [reflexivity | split; [intros y Hy; exact Hy |
                 split; [constructor | split; [intros x0 [] | intros x0 y []]]]]].
      split; [exact Hndr | split; [| split]].
      + apply nodup_cons_fresh; [first [exact Hndw | rewrite Ew; exact Hndw] |].
        intros y Hy. cbn [k w_ord]. apply Hbw. first [exact Hy | rewrite Ew; exact Hy].
      + intros y Hy. rewrite bump_ord_next. specialize (Hbr y Hy). lia.
      + intros w [<- | Hw0]; rewrite bump_ord_next; [cbn [k w_ord]; lia |].
        assert (H := Hbw w). first [specialize (H Hw0) | rewrite Ew in H; specialize (H Hw0)]. lia.
  Qed.

  (* Headline 7. The ord of a fresh operation is the counter value, which
     is above the ord of every entry of the collection. The operation
     advances the counter by one and keeps the collection well formed. The
     first operation takes the ord 0 and opens the counter at 1. A
     re-issued persistent operation keeps its first-round ord instead
     (headline 11, queue_reissue_keeps_ord). *)
  Theorem queue_ord_fresh :
    forall st, wf st ->
      (forall x, In x (q_records st) -> r_ord x < bump_ord (q_counter st)) /\
      (forall x, In x (q_waiters st) -> w_ord x < bump_ord (q_counter st)) /\
      (forall rank v pers,
         wf (o_state (produce st rank v pers)) /\
         q_counter (o_state (produce st rank v pers)) = Some (S (bump_ord (q_counter st)))) /\
      (forall rank pat pers peek,
         wf (o_state (consume st rank pat pers peek)) /\
         q_counter (o_state (consume st rank pat pers peek)) = Some (S (bump_ord (q_counter st)))) /\
      (q_counter st = None -> bump_ord (q_counter st) = 0 /\ bump_next (q_counter st) = Some 1).
  Proof.
    intros st Hwf. pose proof Hwf as [_ [_ [Hbr Hbw]]].
    split; [exact Hbr | split; [exact Hbw | split; [| split]]].
    - intros rank v pers. destruct (produce_wf st rank v pers Hwf) as [H1 [H2 _]].
      split; [exact H1 | exact H2].
    - intros rank pat pers peek. destruct (consume_wf st rank pat pers peek Hwf) as [H1 [H2 _]].
      split; [exact H1 | exact H2].
    - intros ->. split; reflexivity.
  Qed.
  (* ── 9. Headline 4: FIFO over runs ───────────────────────────────── *)

  Inductive qop : Type :=
  | OpProduce : nat -> V -> bool -> qop
  | OpConsume : nat -> P -> bool -> bool -> qop.

  Definition step (st : qstate) (op : qop) : outcome :=
    match op with
    | OpProduce rank v pers => produce st rank v pers
    | OpConsume rank pat pers peek => consume st rank pat pers peek
    end.

  (* A run of fresh operations: each operation in its own transaction. The
     second component is the list of the records that the run removed, in
     removal order. The re-issue of a non-persistent element is an
     operation of the list, because it takes a fresh ord. A re-issued
     persistent operation keeps its first-round ord (decision 43). Its
     re-entry is a round of run_rounds, and queue_fifo_with_reentries
     gives the removal order with re-entries. *)
  Fixpoint run (st : qstate) (ops : list qop) : qstate * list record :=
    match ops with
    | [] => (st, [])
    | op :: rest =>
        let o := step st op in
        let (st', rem) := run (o_state o) rest in
        (st', removed_of o ++ rem)
    end.

  Lemma step_facts :
    forall st op, wf st ->
      let o := step st op in
      wf (o_state o) /\
      bump_ord (q_counter (o_state o)) = S (bump_ord (q_counter st)) /\
      (forall y, In y (q_records (o_state o)) ->
         In y (q_records st) \/ bump_ord (q_counter st) <= r_ord y) /\
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall x, In x (removed_of o) -> In x (q_records st)) /\
      (forall x y, In x (removed_of o) -> In y (q_records (o_state o)) -> r_ord x < r_ord y).
  Proof.
    intros st [rank v pers | rank pat pers peek] Hwf; cbv zeta; unfold step.
    - destruct (produce_wf st rank v pers Hwf) as [H1 [H2 [H3 H4]]].
      split; [exact H1 | split; [rewrite H2; apply bump_ord_next | split]].
      + intros y Hy. destruct (H3 y Hy) as [Hin | ->]; [left; exact Hin | right; reflexivity].
      + rewrite H4. split; [constructor | split; [intros x [] | intros x y []]].
    - destruct (consume_wf st rank pat pers peek Hwf) as [H1 [H2 [H3 [H4 [H5 H6]]]]].
      split; [exact H1 | split; [rewrite H2; apply bump_ord_next | split]].
      + intros y Hy. left. exact (H3 y Hy).
      + split; [exact H4 | split; [exact H5 | exact H6]].
  Qed.

  Lemma StronglySorted_app :
    forall (A : Type) (R : A -> A -> Prop) l1 l2,
      StronglySorted R l1 -> StronglySorted R l2 ->
      (forall x y, In x l1 -> In y l2 -> R x y) -> StronglySorted R (l1 ++ l2).
  Proof.
    intros A R l1 l2 H1. induction H1 as [| a l1 Hs IH Hall]; intros H2 Hcross; [exact H2 |].
    simpl. constructor.
    - apply IH; [exact H2 |]. intros x y Hx Hy. apply Hcross; [right; exact Hx | exact Hy].
    - apply Forall_app. split; [exact Hall |].
      apply Forall_forall. intros y Hy. apply Hcross; [left; reflexivity | exact Hy].
  Qed.

  (* Headline 4. Over any run of fresh operations from a well-formed
     collection, the records leave in strictly increasing ord. Every
     removed record is older than every record that stays. A removed record
     was in the collection at the start or arrived during the run. A fresh
     produce takes the counter value as the ord of its record, so the
     removal order is the arrival order. Headline 14
     (queue_fifo_with_reentries) adds the re-entries of decision 43. *)
  Theorem queue_fifo :
    forall ops st, wf st ->
      let (st', rem) := run st ops in
      wf st' /\
      StronglySorted (fun a b => r_ord a < r_ord b) rem /\
      (forall x y, In x rem -> In y (q_records st') -> r_ord x < r_ord y) /\
      (forall x, In x rem -> In x (q_records st) \/ bump_ord (q_counter st) <= r_ord x) /\
      (forall y, In y (q_records st') -> In y (q_records st) \/ bump_ord (q_counter st) <= r_ord y) /\
      bump_ord (q_counter st) <= bump_ord (q_counter st').
  Proof.
    induction ops as [| op rest IH]; intros st Hwf.
    - simpl. split; [exact Hwf | split; [constructor | split; [intros x y [] | split;
        [intros x [] | split; [intros y Hy; left; exact Hy | lia]]]]].
    - simpl. destruct (step_facts st op Hwf) as [Hwf1 [Hc1 [Hnew1 [Hs1 [Hfrom1 Hold1]]]]].
      set (st1 := o_state (step st op)) in *.
      specialize (IH st1 Hwf1).
      destruct (run st1 rest) as [st' rem2].
      destruct IH as [Hwf' [Hs2 [Hold2 [Hfrom2 [Hnew2 Hc2]]]]].
      pose proof Hwf as [_ [_ [Hbound _]]].
      split; [exact Hwf' | split; [| split; [| split; [| split]]]].
      + apply StronglySorted_app; [exact Hs1 | exact Hs2 |].
        intros x y Hx Hy. destruct (Hfrom2 y Hy) as [Hy1 | Hy1].
        * exact (Hold1 x y Hx Hy1).
        * specialize (Hbound x (Hfrom1 x Hx)). lia.
      + intros x y Hx Hy. apply in_app_or in Hx as [Hx | Hx].
        * destruct (Hnew2 y Hy) as [Hy1 | Hy1].
          -- exact (Hold1 x y Hx Hy1).
          -- specialize (Hbound x (Hfrom1 x Hx)). lia.
        * exact (Hold2 x y Hx Hy).
      + intros x Hx. apply in_app_or in Hx as [Hx | Hx].
        * left. exact (Hfrom1 x Hx).
        * destruct (Hfrom2 x Hx) as [Hx1 | Hx1].
          -- exact (Hnew1 x Hx1).
          -- right. lia.
      + intros y Hy. destruct (Hnew2 y Hy) as [Hy1 | Hy1].
        * exact (Hnew1 y Hy1).
        * right. lia.
      + lia.
  Qed.

  (* ── 10. Headline 5: a blocked head blocks the rest ───────────────── *)

  Lemma offer_loop_blocked :
    forall fuel fired st r wi w,
      head_index w_ord w_rank (q_waiters st) = Some (wi, w) -> fits (w_pat w) (r_val r) = false ->
      offer_loop fuel fired st r = mkOutcome [SStore r] (store_record st r) None.
  Proof. intros [| fuel] fired st r wi w Hh Hf; cbn [offer_loop]; rewrite Hh, Hf; reflexivity. Qed.

  Lemma receive_loop_blocked :
    forall fuel fired st k hi h,
      head_index r_ord r_rank (q_records st) = Some (hi, h) -> fits (w_pat k) (r_val h) = false ->
      receive_loop fuel fired st k = mkOutcome [SPark k] (park_waiter st k) None.
  Proof. intros [| fuel] fired st k hi h Hh Hf; cbn [receive_loop]; rewrite Hh, Hf; reflexivity. Qed.

  Lemma offer_loop_first :
    forall fuel fired st r w rest,
      o_ops (offer_loop fuel fired st r) = SFireProduce w r :: rest ->
      waiter_head (q_waiters st) = Some w.
  Proof.
    intros [| fuel] fired st r w rest H; cbn [offer_loop] in H; unfold waiter_head, head;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w'] |]; try discriminate;
      destruct (negb (fits (w_pat w') (r_val r))); try discriminate;
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w')) (w_pers w'));
      cbn [o_ops] in H; injection H as -> _; reflexivity.
  Qed.

  Lemma receive_loop_first :
    forall fuel fired st k h rest,
      o_ops (receive_loop fuel fired st k) = SFireConsume k h :: rest ->
      record_head (q_records st) = Some h.
  Proof.
    intros [| fuel] fired st k h rest H; cbn [receive_loop] in H; unfold record_head, head;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h'] |]; try discriminate;
      destruct (negb (fits (w_pat k) (r_val h'))); try discriminate;
      destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h' || w_peek k));
      cbn [o_ops] in H; injection H as -> _; reflexivity.
  Qed.

  (* Headline 5. Only the heads interact. A datum that meets a head waiter
     that it does not match is stored, also when a later waiter matches it.
     A continuation that meets a head record that it does not match parks,
     also when a later record matches it. An element whose side is not
     empty joins the tail. The first fire of a plan is on the head. *)
  Theorem queue_head_blocks :
    (forall st r w,
       q_records st = [] -> waiter_head (q_waiters st) = Some w ->
       fits (w_pat w) (r_val r) = false ->
       offer st r = mkOutcome [SStore r] (store_record st r) None) /\
    (forall st k h,
       q_waiters st = [] -> record_head (q_records st) = Some h ->
       fits (w_pat k) (r_val h) = false ->
       receive st k = mkOutcome [SPark k] (park_waiter st k) None) /\
    (forall st r, q_records st <> [] -> offer st r = mkOutcome [SStore r] (store_record st r) None) /\
    (forall st k, q_waiters st <> [] -> receive st k = mkOutcome [SPark k] (park_waiter st k) None) /\
    (forall st r w rest, o_ops (offer st r) = SFireProduce w r :: rest ->
       waiter_head (q_waiters st) = Some w) /\
    (forall st k h rest, o_ops (receive st k) = SFireConsume k h :: rest ->
       record_head (q_records st) = Some h).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros st r w Hrec Hh Hf. unfold offer. rewrite Hrec.
      unfold waiter_head, head in Hh.
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w'] |] eqn:Eh; [| discriminate].
      injection Hh as ->. exact (offer_loop_blocked _ _ st r wi w Eh Hf).
    - intros st k h Hw Hh Hf. unfold receive. rewrite Hw.
      unfold record_head, head in Hh.
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h'] |] eqn:Eh; [| discriminate].
      injection Hh as ->. exact (receive_loop_blocked _ _ st k hi h Eh Hf).
    - intros st r Hne. unfold offer. destruct (q_records st); [contradiction | reflexivity].
    - intros st k Hne. unfold receive. destruct (q_waiters st); [contradiction | reflexivity].
    - intros st r w rest H. unfold offer in H.
      destruct (q_records st); [| discriminate]. exact (offer_loop_first _ _ st r w rest H).
    - intros st k h rest H. unfold receive in H.
      destruct (q_waiters st); [| discriminate]. exact (receive_loop_first _ _ st k h rest H).
  Qed.

  (* ── 11. Headline 6: the loop bound ───────────────────────────────── *)

  Definition is_fire (s : staged) : bool :=
    match s with SFireProduce _ _ | SFireConsume _ _ => true | _ => false end.

  Definition fires (o : outcome) : nat := length (filter is_fire (o_ops o)).

  Lemma length_remove_at :
    forall (A : Type) i (l : list A) x, nth_error l i = Some x -> length (remove_at i l) = length l - 1.
  Proof.
    intros A i l. revert i. induction l as [| a l IH]; intros [| i] x H; simpl in H; try discriminate.
    - simpl. lia.
    - simpl. rewrite (IH i x H). destruct l; [destruct i; discriminate | simpl; lia].
  Qed.

  Lemma after_fire_round_cap :
    forall n spent stays, after_fire n spent stays = FRoundCap -> fire_round_cap <= n.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; [discriminate |]. destruct stays; [discriminate |].
    destruct (fire_round_cap <=? n) eqn:E; [apply Nat.leb_le; exact E | discriminate].
  Qed.

  Lemma after_fire_continue_lt :
    forall n spent stays, after_fire n spent stays = FContinue -> n < fire_round_cap.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; [discriminate |]. destruct stays; [discriminate |].
    destruct (fire_round_cap <=? n) eqn:E; [discriminate | apply Nat.leb_gt; exact E].
  Qed.

  Lemma after_fire_divergent :
    forall n spent stays, after_fire n spent stays = FDivergent -> spent = false /\ stays = true.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; [discriminate |]. destruct stays; [split; reflexivity |].
    destruct (fire_round_cap <=? n); discriminate.
  Qed.

  Lemma offer_loop_fires :
    forall fuel fired st r,
      fuel + fired = fire_round_cap -> fired < fire_round_cap ->
      let o := offer_loop fuel fired st r in
      fired + fires o <= fire_round_cap /\ fires o <= length (q_waiters st) /\
      (o_reissue o = Some ReissueRoundCap -> fired + fires o = fire_round_cap) /\
      (o_reissue o = Some ReissueDivergent ->
         exists w, In (SFireProduce w r) (o_ops o) /\ w_pers w = true /\
                   (r_pers r = true \/ w_peek w = true)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hsum Hlt; cbv zeta;
      [unfold fire_round_cap in *; lia |].
    unfold offer_loop; fold offer_loop.
    destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |] eqn:Eh.
    2: { unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]]. }
    destruct (head_index_head waiter w_ord w_rank _ _ _ Eh) as [_ Hnth].
    assert (Hlen : 1 <= length (q_waiters st))
      by (destruct (q_waiters st); [destruct wi; discriminate | simpl; lia]).
    destruct (negb (fits (w_pat w) (r_val r))).
    { unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]]. }
    destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)) eqn:Eaf.
    - unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]].
    - (* the continuation *)
      pose proof (after_fire_continue_lt _ _ _ Eaf) as HltS.
      pose proof (after_fire_continue _ _ _ Eaf) as [_ Hstays].
      rewrite Hstays. cbn [o_ops o_reissue].
      set (st1 := mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st)).
      destruct (IH (S fired) st1 r ltac:(lia) HltS) as [H1 [H2 [H3 H4]]].
      assert (Hl1 : length (q_waiters st1) = length (q_waiters st) - 1)
        by (cbn [st1 q_waiters]; exact (length_remove_at waiter wi _ w Hnth)).
      unfold fires in *. cbn [o_ops o_reissue filter is_fire length].
      split; [lia | split; [lia | split]].
      + intros H. specialize (H3 H). lia.
      + intros H. destruct (H4 H) as [w' [Hin Hrest]]. exists w'. split; [right; exact Hin | exact Hrest].
    - unfold fires; cbn. split; [lia | split; [lia | split; intros H; try discriminate]].
      exists w. split; [left; reflexivity |].
      destruct (after_fire_divergent _ _ _ Eaf) as [Hsp Hst]. split; [exact Hst |].
      apply andb_false_iff in Hsp as [Hp | Hp]; apply negb_false_iff in Hp; [left | right]; exact Hp.
    - pose proof (after_fire_round_cap _ _ _ Eaf) as Hcap.
      unfold fires; cbn. split; [lia | split; [lia | split; intros H; [lia | discriminate]]].
  Qed.

  Lemma receive_loop_fires :
    forall fuel fired st k,
      fuel + fired = fire_round_cap -> fired < fire_round_cap ->
      let o := receive_loop fuel fired st k in
      fired + fires o <= fire_round_cap /\ fires o <= length (q_records st) /\
      (o_reissue o = Some ReissueRoundCap -> fired + fires o = fire_round_cap) /\
      (o_reissue o = Some ReissueDivergent ->
         w_pers k = true /\
         exists h, In (SFireConsume k h) (o_ops o) /\ (r_pers h = true \/ w_peek k = true)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hsum Hlt; cbv zeta;
      [unfold fire_round_cap in *; lia |].
    unfold receive_loop; fold receive_loop.
    destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    2: { unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]]. }
    destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [_ Hnth].
    assert (Hlen : 1 <= length (q_records st))
      by (destruct (q_records st); [destruct hi; discriminate | simpl; lia]).
    destruct (negb (fits (w_pat k) (r_val h))).
    { unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]]. }
    destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)) eqn:Eaf.
    - unfold fires; cbn. split; [lia | split; [lia | split; intros H; discriminate]].
    - pose proof (after_fire_continue_lt _ _ _ Eaf) as HltS.
      pose proof (after_fire_continue _ _ _ Eaf) as [_ Hstays].
      rewrite Hstays. cbn [o_ops o_reissue].
      set (st1 := mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st)).
      destruct (IH (S fired) st1 k ltac:(lia) HltS) as [H1 [H2 [H3 H4]]].
      assert (Hl1 : length (q_records st1) = length (q_records st) - 1)
        by (cbn [st1 q_records]; exact (length_remove_at record hi _ h Hnth)).
      unfold fires in *. cbn [o_ops o_reissue filter is_fire length].
      split; [lia | split; [lia | split]].
      + intros H. specialize (H3 H). lia.
      + intros H. destruct (H4 H) as [Hk [h' [Hin Hrest]]].
        split; [exact Hk | exists h'; split; [right; exact Hin | exact Hrest]].
    - unfold fires; cbn. split; [lia | split; [lia | split; intros H; try discriminate]].
      destruct (after_fire_divergent _ _ _ Eaf) as [Hsp Hst].
      apply negb_false_iff in Hsp. split; [exact Hsp |].
      exists h. split; [left; reflexivity |].
      apply orb_true_iff in Hst as [Hp | Hp]; [left | right]; exact Hp.
    - pose proof (after_fire_round_cap _ _ _ Eaf) as Hcap.
      unfold fires; cbn. split; [lia | split; [lia | split; intros H; [lia | discriminate]]].
  Qed.

  (* Headline 6. A plan fires at most FIRE_ROUND_CAP times, and at most once
     for each entry of the other side. A plan that stops at the cap has
     fired exactly FIRE_ROUND_CAP times. A divergent re-issue (A10) happens
     only for a persistent partner and a surviving incoming side. Every
     other plan completes in its transaction. *)
  Theorem queue_loop_bounded_or_reissues :
    forall st r k,
      fires (offer st r) <= fire_round_cap /\ fires (offer st r) <= length (q_waiters st) /\
      (o_reissue (offer st r) = Some ReissueRoundCap -> fires (offer st r) = fire_round_cap) /\
      (o_reissue (offer st r) = Some ReissueDivergent ->
         exists w, In (SFireProduce w r) (o_ops (offer st r)) /\ w_pers w = true /\
                   (r_pers r = true \/ w_peek w = true)) /\
      fires (receive st k) <= fire_round_cap /\ fires (receive st k) <= length (q_records st) /\
      (o_reissue (receive st k) = Some ReissueRoundCap -> fires (receive st k) = fire_round_cap) /\
      (o_reissue (receive st k) = Some ReissueDivergent ->
         w_pers k = true /\
         exists h, In (SFireConsume k h) (o_ops (receive st k)) /\ (r_pers h = true \/ w_peek k = true)).
  Proof.
    intros st r k.
    assert (Hoffer : fires (offer st r) <= fire_round_cap /\ fires (offer st r) <= length (q_waiters st) /\
      (o_reissue (offer st r) = Some ReissueRoundCap -> fires (offer st r) = fire_round_cap) /\
      (o_reissue (offer st r) = Some ReissueDivergent ->
         exists w, In (SFireProduce w r) (o_ops (offer st r)) /\ w_pers w = true /\
                   (r_pers r = true \/ w_peek w = true))).
    { unfold offer. destruct (q_records st).
      - pose proof (offer_loop_fires fire_round_cap 0 st r (Nat.add_0_r _)
                      ltac:(unfold fire_round_cap; lia)) as H.
        cbv zeta in H. rewrite !Nat.add_0_l in H. exact H.
      - unfold fires; cbn. split; [unfold fire_round_cap; lia | split; [lia | split; intros H; discriminate]]. }
    assert (Hreceive : fires (receive st k) <= fire_round_cap /\ fires (receive st k) <= length (q_records st) /\
      (o_reissue (receive st k) = Some ReissueRoundCap -> fires (receive st k) = fire_round_cap) /\
      (o_reissue (receive st k) = Some ReissueDivergent ->
         w_pers k = true /\
         exists h, In (SFireConsume k h) (o_ops (receive st k)) /\ (r_pers h = true \/ w_peek k = true))).
    { unfold receive. destruct (q_waiters st).
      - pose proof (receive_loop_fires fire_round_cap 0 st k (Nat.add_0_r _)
                      ltac:(unfold fire_round_cap; lia)) as H.
        cbv zeta in H. rewrite !Nat.add_0_l in H. exact H.
      - unfold fires; cbn. split; [unfold fire_round_cap; lia | split; [lia | split; intros H; discriminate]]. }
    destruct Hoffer as [A1 [A2 [A3 A4]]]. destruct Hreceive as [B1 [B2 [B3 B4]]].
    exact (conj A1 (conj A2 (conj A3 (conj A4 (conj B1 (conj B2 (conj B3 B4))))))).
  Qed.
  (* ── 12. Headline 8: the plan ignores the list order ─────────────── *)

  Definition q_equiv (a b : qstate) : Prop :=
    Permutation (q_records a) (q_records b) /\ Permutation (q_waiters a) (q_waiters b) /\
    q_counter a = q_counter b.

  Lemma keys_unique_incl :
    forall (E : Type) (ord rank : E -> nat) l1 l2,
      (forall x, In x l2 -> In x l1) -> keys_unique ord rank l1 -> keys_unique ord rank l2.
  Proof. intros E ord rank l1 l2 Hincl Hk x y Hx Hy. apply Hk; apply Hincl; assumption. Qed.

  Lemma head_perm :
    forall (E : Type) (ord rank : E -> nat) l1 l2,
      Permutation l1 l2 -> keys_unique ord rank l1 -> head ord rank l1 = head ord rank l2.
  Proof.
    intros E ord rank l1 l2 Hp Hk. destruct l1 as [| x l1'].
    - apply Permutation_nil in Hp. subst. reflexivity.
    - destruct (head_exists E ord rank (x :: l1') ltac:(discriminate)) as [h Hh].
      destruct (head_spec E ord rank _ h Hh) as [Hin Hmin].
      rewrite Hh. symmetry. apply head_unique.
      + apply (keys_unique_incl E ord rank (x :: l1')); [| exact Hk].
        intros y Hy. apply (Permutation_in y (Permutation_sym Hp)). exact Hy.
      + exact (Permutation_in h Hp Hin).
      + intros y Hy. apply Hmin. exact (Permutation_in y (Permutation_sym Hp) Hy).
  Qed.

  Lemma remove_at_perm2 :
    forall (A : Type) l1 l2 i1 i2 (x : A),
      Permutation l1 l2 -> nth_error l1 i1 = Some x -> nth_error l2 i2 = Some x ->
      Permutation (remove_at i1 l1) (remove_at i2 l2).
  Proof.
    intros A l1 l2 i1 i2 x Hp H1 H2.
    apply (Permutation_cons_inv (a := x)).
    eapply perm_trans; [apply Permutation_sym; exact (remove_at_perm A i1 l1 x H1) |].
    eapply perm_trans; [exact Hp | exact (remove_at_perm A i2 l2 x H2)].
  Qed.

  Lemma head_index_perm :
    forall (E : Type) (ord rank : E -> nat) l1 l2,
      Permutation l1 l2 -> keys_unique ord rank l1 ->
      match head_index ord rank l1, head_index ord rank l2 with
      | Some (i1, e1), Some (i2, e2) =>
          e1 = e2 /\ nth_error l1 i1 = Some e1 /\ nth_error l2 i2 = Some e2
      | None, None => True
      | _, _ => False
      end.
  Proof.
    intros E ord rank l1 l2 Hp Hk. pose proof (head_perm E ord rank l1 l2 Hp Hk) as Hh.
    unfold head in Hh.
    destruct (head_index ord rank l1) as [[i1 e1] |] eqn:E1;
      destruct (head_index ord rank l2) as [[i2 e2] |] eqn:E2; cbn in Hh; try discriminate; [| exact I].
    injection Hh as <-. split; [reflexivity |].
    split; [exact (proj2 (head_index_head E ord rank _ _ _ E1)) | exact (proj2 (head_index_head E ord rank _ _ _ E2))].
  Qed.

  Lemma offer_loop_perm :
    forall fuel fired st1 st2 r,
      q_equiv st1 st2 -> keys_unique w_ord w_rank (q_waiters st1) ->
      o_ops (offer_loop fuel fired st1 r) = o_ops (offer_loop fuel fired st2 r) /\
      o_reissue (offer_loop fuel fired st1 r) = o_reissue (offer_loop fuel fired st2 r) /\
      q_equiv (o_state (offer_loop fuel fired st1 r)) (o_state (offer_loop fuel fired st2 r)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st1 st2 r [Hpr [Hpw Hc]] Hk;
      unfold offer_loop; fold offer_loop;
      pose proof (head_index_perm waiter w_ord w_rank _ _ Hpw Hk) as Hhp;
      destruct (head_index w_ord w_rank (q_waiters st1)) as [[i1 w1] |];
      destruct (head_index w_ord w_rank (q_waiters st2)) as [[i2 w2] |]; try contradiction.
    all: try (cbn [o_ops o_reissue o_state]; unfold store_record, q_equiv; cbn [q_records q_waiters q_counter];
              split; [reflexivity | split; [reflexivity |]];
              split; [apply perm_skip; exact Hpr | split; [exact Hpw | exact Hc]]).
    all: destruct Hhp as [<- [Hn1 Hn2]].
    all: destruct (negb (fits (w_pat w1) (r_val r))).
    all: try (cbn [o_ops o_reissue o_state]; unfold store_record, q_equiv; cbn [q_records q_waiters q_counter];
              split; [reflexivity | split; [reflexivity |]];
              split; [apply perm_skip; exact Hpr | split; [exact Hpw | exact Hc]]).
    all: assert (Heq1 : q_equiv
                   (if w_pers w1 then st1 else mkQ (q_records st1) (remove_at i1 (q_waiters st1)) (q_counter st1))
                   (if w_pers w1 then st2 else mkQ (q_records st2) (remove_at i2 (q_waiters st2)) (q_counter st2)))
           by (destruct (w_pers w1); [split; [exact Hpr | split; [exact Hpw | exact Hc]] |];
               unfold q_equiv; cbn [q_records q_waiters q_counter];
               split; [exact Hpr | split; [exact (remove_at_perm2 waiter _ _ i1 i2 w1 Hpw Hn1 Hn2) | exact Hc]]).
    all: assert (Hk1 : keys_unique w_ord w_rank
                   (q_waiters (if w_pers w1 then st1 else mkQ (q_records st1) (remove_at i1 (q_waiters st1)) (q_counter st1))))
           by (destruct (w_pers w1); [exact Hk |];
               apply (keys_unique_incl waiter w_ord w_rank (q_waiters st1)); [| exact Hk];
               cbn [q_waiters]; apply remove_at_in).
    all: destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w1)) (w_pers w1));
      cbn [o_ops o_reissue o_state]; try (split; [reflexivity | split; [reflexivity | exact Heq1]]).
    destruct (IH (S fired) _ _ r Heq1 Hk1) as [H1 [H2 H3]].
    rewrite H1. split; [reflexivity | split; [exact H2 | exact H3]].
  Qed.

  Lemma receive_loop_perm :
    forall fuel fired st1 st2 k,
      q_equiv st1 st2 -> keys_unique r_ord r_rank (q_records st1) ->
      o_ops (receive_loop fuel fired st1 k) = o_ops (receive_loop fuel fired st2 k) /\
      o_reissue (receive_loop fuel fired st1 k) = o_reissue (receive_loop fuel fired st2 k) /\
      q_equiv (o_state (receive_loop fuel fired st1 k)) (o_state (receive_loop fuel fired st2 k)).
  Proof.
    induction fuel as [| fuel IH]; intros fired st1 st2 k [Hpr [Hpw Hc]] Hk;
      unfold receive_loop; fold receive_loop;
      pose proof (head_index_perm record r_ord r_rank _ _ Hpr Hk) as Hhp;
      destruct (head_index r_ord r_rank (q_records st1)) as [[i1 h1] |];
      destruct (head_index r_ord r_rank (q_records st2)) as [[i2 h2] |]; try contradiction.
    all: try (cbn [o_ops o_reissue o_state]; unfold park_waiter, q_equiv; cbn [q_records q_waiters q_counter];
              split; [reflexivity | split; [reflexivity |]];
              split; [exact Hpr | split; [apply perm_skip; exact Hpw | exact Hc]]).
    all: destruct Hhp as [<- [Hn1 Hn2]].
    all: destruct (negb (fits (w_pat k) (r_val h1))).
    all: try (cbn [o_ops o_reissue o_state]; unfold park_waiter, q_equiv; cbn [q_records q_waiters q_counter];
              split; [reflexivity | split; [reflexivity |]];
              split; [exact Hpr | split; [apply perm_skip; exact Hpw | exact Hc]]).
    all: assert (Heq1 : q_equiv
                   (if r_pers h1 || w_peek k then st1 else mkQ (remove_at i1 (q_records st1)) (q_waiters st1) (q_counter st1))
                   (if r_pers h1 || w_peek k then st2 else mkQ (remove_at i2 (q_records st2)) (q_waiters st2) (q_counter st2)))
           by (destruct (r_pers h1 || w_peek k); [split; [exact Hpr | split; [exact Hpw | exact Hc]] |];
               unfold q_equiv; cbn [q_records q_waiters q_counter];
               split; [exact (remove_at_perm2 record _ _ i1 i2 h1 Hpr Hn1 Hn2) | split; [exact Hpw | exact Hc]]).
    all: assert (Hk1 : keys_unique r_ord r_rank
                   (q_records (if r_pers h1 || w_peek k then st1 else mkQ (remove_at i1 (q_records st1)) (q_waiters st1) (q_counter st1))))
           by (destruct (r_pers h1 || w_peek k); [exact Hk |];
               apply (keys_unique_incl record r_ord r_rank (q_records st1)); [| exact Hk];
               cbn [q_records]; apply remove_at_in).
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h1 || w_peek k));
      cbn [o_ops o_reissue o_state]; try (split; [reflexivity | split; [reflexivity | exact Heq1]]).
    destruct (IH (S fired) _ _ k Heq1 Hk1) as [H1 [H2 H3]].
    rewrite H1. split; [reflexivity | split; [exact H2 | exact H3]].
  Qed.

  (* Headline 8. The plan of an operation depends on the entries of the
     collection, not on the order of the lists of the staged view (the hot
     store inserts at the front, and a restart can load another order). Two
     views with permuted lists stage the same operations and reach permuted
     views. The premise is that no two distinct entries of a side share one
     (ord, rank), which wf implies. *)
  Theorem queue_plan_permutation_invariant :
    forall st1 st2,
      q_equiv st1 st2 ->
      keys_unique r_ord r_rank (q_records st1) -> keys_unique w_ord w_rank (q_waiters st1) ->
      (forall rank v pers,
         o_ops (produce st1 rank v pers) = o_ops (produce st2 rank v pers) /\
         o_reissue (produce st1 rank v pers) = o_reissue (produce st2 rank v pers) /\
         q_equiv (o_state (produce st1 rank v pers)) (o_state (produce st2 rank v pers))) /\
      (forall rank pat pers peek,
         o_ops (consume st1 rank pat pers peek) = o_ops (consume st2 rank pat pers peek) /\
         o_reissue (consume st1 rank pat pers peek) = o_reissue (consume st2 rank pat pers peek) /\
         q_equiv (o_state (consume st1 rank pat pers peek)) (o_state (consume st2 rank pat pers peek))).
  Proof.
    intros st1 st2 [Hpr [Hpw Hc]] Hkr Hkw.
    assert (Hb : q_equiv (bumped st1) (bumped st2))
      by (unfold bumped, q_equiv; cbn [q_records q_waiters q_counter];
          split; [exact Hpr | split; [exact Hpw | rewrite Hc; reflexivity]]).
    split.
    - intros rank v pers. unfold produce, offer. rewrite Hc. cbn [q_records bumped].
      destruct (q_records st1) as [| x xs] eqn:E1; destruct (q_records st2) as [| y ys] eqn:E2.
      + exact (offer_loop_perm fire_round_cap 0 _ _ _ Hb Hkw).
      + apply Permutation_nil in Hpr. discriminate.
      + apply Permutation_sym, Permutation_nil in Hpr. discriminate.
      + cbn [o_ops o_reissue o_state]. unfold store_record, q_equiv, bumped.
        cbn [q_records q_waiters q_counter]. rewrite E1, E2.
        split; [reflexivity | split; [reflexivity | split; [apply perm_skip; exact Hpr | split; [exact Hpw | rewrite Hc; reflexivity]]]].
    - intros rank pat pers peek. unfold consume, receive. rewrite Hc. cbn [q_waiters bumped].
      destruct (q_waiters st1) as [| x xs] eqn:E1; destruct (q_waiters st2) as [| y ys] eqn:E2.
      + exact (receive_loop_perm fire_round_cap 0 _ _ _ Hb Hkr).
      + apply Permutation_nil in Hpw. discriminate.
      + apply Permutation_sym, Permutation_nil in Hpw. discriminate.
      + cbn [o_ops o_reissue o_state]. unfold park_waiter, q_equiv, bumped.
        cbn [q_records q_waiters q_counter]. rewrite E1, E2.
        split; [reflexivity | split; [reflexivity | split; [exact Hpr | split; [apply perm_skip; exact Hpw | rewrite Hc; reflexivity]]]].
  Qed.

  (* ── 13. The legacy waiter scan ───────────────────────────────────── *)

  Fixpoint first_fitting (l : list waiter) (r : record) (i : nat) : option (nat * waiter) :=
    match l with
    | [] => None
    | w :: rest => if fits (w_pat w) (r_val r) then Some (i, w) else first_fitting rest r (S i)
    end.

  (* The legacy plan: the datum fires the first waiter in list order that
     it matches, whatever the ord of that waiter. *)
  Definition legacy_offer (st : qstate) (r : record) : outcome :=
    match q_records st with
    | _ :: _ => mkOutcome [SStore r] (store_record st r) None
    | [] =>
        match first_fitting (q_waiters st) r 0 with
        | None => mkOutcome [SStore r] (store_record st r) None
        | Some (i, w) =>
            mkOutcome [SFireProduce w r]
              (if w_pers w then st else mkQ (q_records st) (remove_at i (q_waiters st)) (q_counter st)) None
        end
    end.

  (* ── 14. Decision 43: the definitions of re-entry ──────────────────── *)

  (* An element in flight (design 43.4): a persistent datum or a
     persistent continuation whose operation re-issued (A10 or I-21) and
     did not re-enter yet. The element keeps the ord of its first round.
     So the list F of the elements in flight holds records and waiters
     with their first-round ords. *)
  Inductive in_flight : Type :=
  | FlightRecord : record -> in_flight
  | FlightWaiter : waiter -> in_flight.

  Definition flight_ord (f : in_flight) : nat :=
    match f with FlightRecord r => r_ord r | FlightWaiter w => w_ord w end.

  Definition flight_pers (f : in_flight) : bool :=
    match f with FlightRecord r => r_pers r | FlightWaiter w => w_pers w end.

  (* operation_ord: the ord of a round. A re-issued operation takes its
     first-round ord when that ord is below the value of the bump of the
     round. A fresh operation takes the value of the bump. None is
     Inconsistent. *)
  Definition operation_ord (reissued : option nat) (bump : nat) : option nat :=
    match reissued with
    | None => Some bump
    | Some o => if o <? bump then Some o else None
    end.

  (* QueueDiscipline::precedes: the lesser ord interacts first. *)
  Definition ord_precedes (o head : nat) : bool := o <? head.

  (* Cx::at_head for a re-entered element (QD-9). The element is
     at the head when its side is empty or when its ord precedes the ord of
     the head. A stored entry with the ord of the element is Inconsistent
     (None). *)
  Definition reentry_at_head (o : nat) (head : option nat) : option bool :=
    match head with
    | None => Some true
    | Some h => if o =? h then None else Some (ord_precedes o h)
    end.

  (* The plan of a re-entered datum r (offer_head): the offer loop
     from the head, or a store at its place. *)
  Definition reenter_offer (st : qstate) (r : record) : option outcome :=
    match reentry_at_head (r_ord r) (option_map r_ord (record_head (q_records st))) with
    | None => None
    | Some true => Some (offer_loop fire_round_cap 0 st r)
    | Some false => Some (mkOutcome [SStore r] (store_record st r) None)
    end.

  (* The plan of a re-entered continuation k (receive_head): the
     receive loop from the head, or a park at its place. *)
  Definition reenter_receive (st : qstate) (k : waiter) : option outcome :=
    match reentry_at_head (w_ord k) (option_map w_ord (waiter_head (q_waiters st))) with
    | None => None
    | Some true => Some (receive_loop fire_round_cap 0 st k)
    | Some false => Some (mkOutcome [SPark k] (park_waiter st k) None)
    end.

  Definition reenter_plan (st : qstate) (f : in_flight) : option outcome :=
    match f with
    | FlightRecord r => reenter_offer st r
    | FlightWaiter k => reenter_receive st k
    end.

  (* The round of a re-issued persistent operation (prepare_produce and
     prepare_consume with a reissued_ord). The round bumps the counter and
     takes the ord of operation_ord. Then it plans the record or the
     continuation with that ord. *)
  Definition reenter_produce (st : qstate) (r : record) : option outcome :=
    match operation_ord (Some (r_ord r)) (bump_ord (q_counter st)) with
    | None => None
    | Some o => reenter_offer (bumped st) (mkRecord o (r_rank r) (r_val r) (r_pers r))
    end.

  Definition reenter_consume (st : qstate) (k : waiter) : option outcome :=
    match operation_ord (Some (w_ord k)) (bump_ord (q_counter st)) with
    | None => None
    | Some o => reenter_receive (bumped st) (mkWaiter o (w_rank k) (w_pat k) (w_pers k) (w_peek k))
    end.

  Definition reenter (st : qstate) (f : in_flight) : option outcome :=
    match f with
    | FlightRecord r => reenter_produce st r
    | FlightWaiter k => reenter_consume st k
    end.

  (* The reissued_ord of a re-issue: the ord of a persistent element, and
     none for a non-persistent element (op.persistent.then_some(ord)). *)
  Definition reissued_ord (x : in_flight) : option nat :=
    if flight_pers x then Some (flight_ord x) else None.

  (* F after a round of the element x with the outcome o. A re-issue that
     carries an ord puts x in flight. A re-issue without an ord runs again
     as a fresh operation (a RoundFresh round), so F does not hold it. *)
  Definition flight_after (x : in_flight) (o : outcome) (F : list in_flight) : list in_flight :=
    match o_reissue o, reissued_ord x with
    | Some _, Some _ => x :: F
    | _, _ => F
    end.

  (* wf' (design 43.4). The collection is well formed, and every element
     in flight is persistent. The first-round ords of F are distinct and
     below the counter value. No stored entry has the ord of an element in
     flight. *)
  Definition wf' (st : qstate) (F : list in_flight) : Prop :=
    wf st /\ NoDup (map flight_ord F) /\
    (forall f, In f F -> flight_pers f = true /\ flight_ord f < bump_ord (q_counter st)) /\
    (forall f y, In f F -> In y (q_records st) -> flight_ord f <> r_ord y) /\
    (forall f y, In f F -> In y (q_waiters st) -> flight_ord f <> w_ord y).

  (* An element in flight shadows the heads h and w when it is older than
     the head of its side. *)
  Definition shadows (f : in_flight) (h : record) (w : waiter) : Prop :=
    match f with
    | FlightRecord x => r_ord x < r_ord h
    | FlightWaiter x => w_ord x < w_ord w
    end.

  (* SHADOW (design 43.4): when the two heads exist and match, an element
     in flight shadows them. INV (no_pending_match) is SHADOW with F = []
     (queue_quiescent_inv). *)
  Definition SHADOW (st : qstate) (F : list in_flight) : Prop :=
    match record_head (q_records st), waiter_head (q_waiters st) with
    | Some h, Some w => fits (w_pat w) (r_val h) = true -> exists f, In f F /\ shadows f h w
    | _, _ => True
    end.

  (* A staged operation that names the element f: its store, its park, or
     one of its fires. *)
  Definition stages (f : in_flight) (s : staged) : Prop :=
    match f, s with
    | FlightRecord r, SStore r' => r' = r
    | FlightRecord r, SFireProduce _ r' => r' = r
    | FlightWaiter k, SPark k' => k' = k
    | FlightWaiter k, SFireConsume k' _ => k' = k
    | _, _ => False
    end.

  (* A round of a run with re-entries. RoundFresh runs an operation with a
     fresh ord: an operation of the program, or the re-issue of a
     non-persistent element. RoundReenter i runs the re-entry of the
     element at index i of F. *)
  Inductive round : Type :=
  | RoundFresh : qop -> round
  | RoundReenter : nat -> round.

  (* The element of a fresh operation: its record or its continuation with
     the ord of the bump. *)
  Definition fresh_element (st : qstate) (op : qop) : in_flight :=
    match op with
    | OpProduce rank v pers => FlightRecord (mkRecord (bump_ord (q_counter st)) rank v pers)
    | OpConsume rank pat pers peek => FlightWaiter (mkWaiter (bump_ord (q_counter st)) rank pat pers peek)
    end.

  (* The plan of a round: its element, its outcome, and F without the
     element. None when F has no element at the index, or when the round
     is Inconsistent. *)
  Definition round_plan (st : qstate) (F : list in_flight) (rd : round)
    : option (in_flight * outcome * list in_flight) :=
    match rd with
    | RoundFresh op => Some (fresh_element st op, step st op, F)
    | RoundReenter i =>
        match nth_error F i with
        | None => None
        | Some f =>
            match reenter st f with
            | None => None
            | Some o => Some (f, o, remove_at i F)
            end
        end
    end.

  (* A run with re-entries: each round in its own transaction. The result
     is the final state, the final F, and the records that the run
     removed, in removal order. A round without a plan changes nothing. *)
  Fixpoint run_rounds (st : qstate) (F : list in_flight) (rounds : list round)
    : qstate * list in_flight * list record :=
    match rounds with
    | [] => (st, F, [])
    | rd :: rest =>
        match round_plan st F rd with
        | None => run_rounds st F rest
        | Some (x, o, F0) =>
            let '(st', F', rem) := run_rounds (o_state o) (flight_after x o F0) rest in
            (st', F', removed_of o ++ rem)
        end
    end.

  (* ── 15. Decision 43: the loops from any staged view ───────────────── *)

  (* The head waiter does not match the datum r, or no waiter waits. *)
  Definition no_match_head_waiter (ws : list waiter) (r : record) : Prop :=
    match waiter_head ws with Some w => fits (w_pat w) (r_val r) = false | None => True end.

  (* The head record does not match the continuation k, or no record is
     stored. *)
  Definition no_match_head_record (rs : list record) (k : waiter) : Prop :=
    match record_head rs with Some h => fits (w_pat k) (r_val h) = false | None => True end.

  Lemma after_fire_done :
    forall n spent stays, after_fire n spent stays = FDone -> spent = true.
  Proof.
    intros n spent stays H. unfold after_fire in H.
    destruct spent; [reflexivity |]. destruct stays; [discriminate |].
    destruct (fire_round_cap <=? n); discriminate.
  Qed.

  (* The end of an offer loop from any staged view. The records side can
     hold entries (a re-entry at the head). The datum is stored with no
     match at the head waiter, or a fire spends it, or it re-issues. The
     records change only by the store, the waiters only lose entries, and
     the counter stays. *)
  Definition offer_end (st : qstate) (r : record) (o : outcome) : Prop :=
    q_counter (o_state o) = q_counter st /\
    (forall w, In w (q_waiters (o_state o)) -> In w (q_waiters st)) /\
    ((q_records (o_state o) = r :: q_records st /\ o_reissue o = None /\
      no_match_head_waiter (q_waiters (o_state o)) r) \/
     (q_records (o_state o) = q_records st /\ o_reissue o = None /\ r_pers r = false) \/
     (q_records (o_state o) = q_records st /\ o_reissue o <> None)).

  Lemma offer_end_store :
    forall st r, no_match_head_waiter (q_waiters st) r ->
      offer_end st r (mkOutcome [SStore r] (store_record st r) None).
  Proof.
    intros st r Hm. unfold offer_end, store_record.
    cbn [o_state o_reissue q_records q_waiters q_counter].
    split; [reflexivity | split; [intros w0 H0; exact H0 |]].
    left. split; [reflexivity | split; [reflexivity | exact Hm]].
  Qed.

  Lemma offer_end_fired :
    forall st r ops st1 re,
      q_counter st1 = q_counter st -> q_records st1 = q_records st ->
      (forall w, In w (q_waiters st1) -> In w (q_waiters st)) ->
      (re = None -> r_pers r = false) ->
      offer_end st r (mkOutcome ops st1 re).
  Proof.
    intros st r ops st1 re Hc Hr Hw Hsp. unfold offer_end. cbn [o_state o_reissue].
    split; [exact Hc | split; [exact Hw |]]. right.
    destruct re as [c |].
    - right. split; [exact Hr | discriminate].
    - left. split; [exact Hr | split; [reflexivity | exact (Hsp eq_refl)]].
  Qed.

  Lemma offer_end_rest :
    forall st st1 r ops o,
      q_counter st1 = q_counter st -> q_records st1 = q_records st ->
      (forall w, In w (q_waiters st1) -> In w (q_waiters st)) ->
      offer_end st1 r o -> offer_end st r (mkOutcome ops (o_state o) (o_reissue o)).
  Proof.
    intros st st1 r ops o Hc Hr Hw [H1 [H2 H3]]. unfold offer_end. cbn [o_state o_reissue].
    split; [rewrite H1; exact Hc | split; [intros w0 H0; exact (Hw w0 (H2 w0 H0)) |]].
    rewrite Hr in H3. exact H3.
  Qed.

  Lemma offer_fired_state :
    forall st wi (w : waiter),
      let st1 := if w_pers w then st else mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st) in
      q_counter st1 = q_counter st /\ q_records st1 = q_records st /\
      (forall w0, In w0 (q_waiters st1) -> In w0 (q_waiters st)).
  Proof.
    intros st wi w. cbv zeta. destruct (w_pers w); cbn [q_counter q_records q_waiters].
    - split; [reflexivity | split; [reflexivity | intros w0 H0; exact H0]].
    - split; [reflexivity | split; [reflexivity | apply remove_at_in]].
  Qed.

  Lemma offer_loop_end : forall fuel fired st r, offer_end st r (offer_loop fuel fired st r).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r;
      unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |] eqn:Eh.
    2, 4: apply offer_end_store; unfold no_match_head_waiter, waiter_head, head; rewrite Eh; exact I.
    all: destruct (head_index_head waiter w_ord w_rank _ _ _ Eh) as [Hhead _].
    all: destruct (fits (w_pat w) (r_val r)) eqn:Efit; cbn [negb].
    2, 4: apply offer_end_store; unfold no_match_head_waiter, waiter_head; rewrite Hhead; exact Efit.
    all: destruct (offer_fired_state st wi w) as [Hc [Hr Hw]].
    all: destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)) eqn:Eaf.
    (* fuel S, FContinue: the loop goes on with the next head *)
    6: exact (offer_end_rest st _ r _ _ Hc Hr Hw (IH (S fired) _ r)).
    all: apply offer_end_fired; [exact Hc | exact Hr | exact Hw |].
    all: intros Hre; try discriminate.
    all: apply after_fire_done in Eaf; apply andb_true_iff in Eaf as [Ep _].
    all: apply negb_true_iff in Ep; exact Ep.
  Qed.

  (* The end of a receive loop from any staged view (the waiters side can
     hold entries), symmetric to offer_end. *)
  Definition receive_end (st : qstate) (k : waiter) (o : outcome) : Prop :=
    q_counter (o_state o) = q_counter st /\
    (forall x, In x (q_records (o_state o)) -> In x (q_records st)) /\
    ((q_waiters (o_state o) = k :: q_waiters st /\ o_reissue o = None /\
      no_match_head_record (q_records (o_state o)) k) \/
     (q_waiters (o_state o) = q_waiters st /\ o_reissue o = None /\ w_pers k = false) \/
     (q_waiters (o_state o) = q_waiters st /\ o_reissue o <> None)).

  Lemma receive_end_park :
    forall st k, no_match_head_record (q_records st) k ->
      receive_end st k (mkOutcome [SPark k] (park_waiter st k) None).
  Proof.
    intros st k Hm. unfold receive_end, park_waiter.
    cbn [o_state o_reissue q_records q_waiters q_counter].
    split; [reflexivity | split; [intros x0 H0; exact H0 |]].
    left. split; [reflexivity | split; [reflexivity | exact Hm]].
  Qed.

  Lemma receive_end_fired :
    forall st k ops st1 re,
      q_counter st1 = q_counter st -> q_waiters st1 = q_waiters st ->
      (forall x, In x (q_records st1) -> In x (q_records st)) ->
      (re = None -> w_pers k = false) ->
      receive_end st k (mkOutcome ops st1 re).
  Proof.
    intros st k ops st1 re Hc Hw Hr Hsp. unfold receive_end. cbn [o_state o_reissue].
    split; [exact Hc | split; [exact Hr |]]. right.
    destruct re as [c |].
    - right. split; [exact Hw | discriminate].
    - left. split; [exact Hw | split; [reflexivity | exact (Hsp eq_refl)]].
  Qed.

  Lemma receive_end_rest :
    forall st st1 k ops o,
      q_counter st1 = q_counter st -> q_waiters st1 = q_waiters st ->
      (forall x, In x (q_records st1) -> In x (q_records st)) ->
      receive_end st1 k o -> receive_end st k (mkOutcome ops (o_state o) (o_reissue o)).
  Proof.
    intros st st1 k ops o Hc Hw Hr [H1 [H2 H3]]. unfold receive_end. cbn [o_state o_reissue].
    split; [rewrite H1; exact Hc | split; [intros x0 H0; exact (Hr x0 (H2 x0 H0)) |]].
    rewrite Hw in H3. exact H3.
  Qed.

  Lemma receive_fired_state :
    forall st hi (h : record) (k : waiter),
      let st1 := if r_pers h || w_peek k then st
                 else mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st) in
      q_counter st1 = q_counter st /\ q_waiters st1 = q_waiters st /\
      (forall x, In x (q_records st1) -> In x (q_records st)).
  Proof.
    intros st hi h k. cbv zeta. destruct (r_pers h || w_peek k); cbn [q_counter q_records q_waiters].
    - split; [reflexivity | split; [reflexivity | intros x0 H0; exact H0]].
    - split; [reflexivity | split; [reflexivity | apply remove_at_in]].
  Qed.

  Lemma receive_loop_end : forall fuel fired st k, receive_end st k (receive_loop fuel fired st k).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    2, 4: apply receive_end_park; unfold no_match_head_record, record_head, head; rewrite Eh; exact I.
    all: destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [Hhead _].
    all: destruct (fits (w_pat k) (r_val h)) eqn:Efit; cbn [negb].
    2, 4: apply receive_end_park; unfold no_match_head_record, record_head; rewrite Hhead; exact Efit.
    all: destruct (receive_fired_state st hi h k) as [Hc [Hw Hr]].
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)) eqn:Eaf.
    (* fuel S, FContinue: the loop goes on with the next head *)
    6: exact (receive_end_rest st _ k _ _ Hc Hw Hr (IH (S fired) _ k)).
    all: apply receive_end_fired; [exact Hc | exact Hw | exact Hr |].
    all: intros Hre; try discriminate.
    all: apply after_fire_done in Eaf; apply negb_true_iff in Eaf; exact Eaf.
  Qed.

  (* receive_loop_removed without the premise that no waiter waits. The
     removed records leave from the head in increasing ord. Each removed
     record is older than every record that stays. *)
  Lemma receive_loop_removed_any :
    forall fuel fired st k,
      NoDup (map r_ord (q_records st)) ->
      let o := receive_loop fuel fired st k in
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall x, In x (removed_of o) -> In x (q_records st)) /\
      (forall x y, In x (removed_of o) -> In y (q_records (o_state o)) -> r_ord x < r_ord y) /\
      NoDup (map r_ord (q_records (o_state o))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hnd; cbv zeta;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    all: try (unfold removed_of; cbn [o_ops o_state flat_map park_waiter q_records app];
              split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]).
    all: destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [Hhead Hnth].
    all: destruct (head_spec record r_ord r_rank _ _ Hhead) as [Hin Hmin].
    all: destruct (fits (w_pat k) (r_val h)) eqn:Efit; cbn [negb].
    all: try (unfold removed_of; cbn [o_ops o_state flat_map park_waiter q_records app];
              split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]).
    (* The removed head is older than every record that stays. *)
    all: assert (Hlt : forall y, In y (remove_at hi (q_records st)) -> r_ord h < r_ord y)
           by (intros y Hy; pose proof (Hmin y (remove_at_in record hi _ y Hy)) as Hle;
               pose proof (remove_at_nodup_other record r_ord hi _ h y Hnth Hnd Hy) as Hne;
               unfold key_le in Hle; lia).
    all: assert (Hnd1 : NoDup (map r_ord (remove_at hi (q_records st))))
           by (apply remove_at_nodup; exact Hnd).
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)) eqn:Eaf.
    all: try (destruct (r_pers h || w_peek k) eqn:Estays;
              unfold removed_of; cbn [o_ops o_state flat_map q_records app]; rewrite ?Estays;
              cbn [app];
              [ split; [constructor | split; [intros x [] | split; [intros x y [] | exact Hnd]]]
              | split; [repeat constructor |];
                split; [intros x [-> | []]; exact Hin |];
                split; [intros x y [-> | []] Hy; exact (Hlt y Hy) | exact Hnd1] ]; fail).
    (* The continuation case: the record leaves, and the loop goes on. *)
    all: pose proof (after_fire_continue _ _ _ Eaf) as [_ Hstays].
    all: unfold removed_of; cbn [o_ops o_state flat_map q_records app]; rewrite Hstays; cbn [app].
    set (st1 := mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st)).
    destruct (IH (S fired) st1 k Hnd1) as [Hs [Hfrom [Hold Hnd']]].
    fold (removed_of (receive_loop fuel (S fired) st1 k)).
    pose proof (receive_loop_end fuel (S fired) st1 k) as [_ [Hsub _]].
    split; [constructor; [exact Hs |] | split; [| split; [| exact Hnd']]].
    - apply Forall_forall. intros x Hx. apply Hlt. exact (Hfrom x Hx).
    - intros x [-> | Hx]; [exact Hin | exact (remove_at_in record hi _ x (Hfrom x Hx))].
    - intros x y [-> | Hx] Hy; [apply Hlt; exact (Hsub y Hy) | exact (Hold x y Hx Hy)].
  Qed.

  Lemma offer_loop_stages :
    forall fuel fired st r s, In s (o_ops (offer_loop fuel fired st r)) -> stages (FlightRecord r) s.
  Proof.
    induction fuel as [| fuel IH]; intros fired st r s;
      unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |].
    all: try (destruct (negb (fits (w_pat w) (r_val r)))).
    all: try (destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w))).
    all: cbn [o_ops]; intros Hs.
    all: destruct Hs as [<- | Hs]; [reflexivity |].
    all: first [destruct Hs | exact (IH _ _ _ _ Hs)].
  Qed.

  Lemma receive_loop_stages :
    forall fuel fired st k s, In s (o_ops (receive_loop fuel fired st k)) -> stages (FlightWaiter k) s.
  Proof.
    induction fuel as [| fuel IH]; intros fired st k s;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |].
    all: try (destruct (negb (fits (w_pat k) (r_val h)))).
    all: try (destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k))).
    all: cbn [o_ops]; intros Hs.
    all: destruct Hs as [<- | Hs]; [reflexivity |].
    all: first [destruct Hs | exact (IH _ _ _ _ Hs)].
  Qed.

  (* ── 16. Decision 43: heads, places and plans ──────────────────────── *)

  (* An entry older than every entry of a side is the head of the side
     with the entry. *)
  Lemma head_cons_least :
    forall (E : Type) (ord rank : E -> nat) e l,
      NoDup (map ord l) -> (forall x, In x l -> ord e < ord x) ->
      head ord rank (e :: l) = Some e.
  Proof.
    intros E ord rank e l Hnd Hlt. apply head_unique.
    - apply nodup_keys_unique. cbn [map]. apply NoDup_cons_iff. split; [| exact Hnd].
      intros Hin. apply in_map_iff in Hin as [x [Hx Hin]]. specialize (Hlt x Hin). lia.
    - left. reflexivity.
    - intros x [<- | Hx]; [apply key_le_refl | unfold key_le; left; exact (Hlt x Hx)].
  Qed.

  (* An entry behind the head of a side does not change the head. *)
  Lemma head_cons_behind :
    forall (E : Type) (ord rank : E -> nat) e l h,
      NoDup (map ord (e :: l)) -> head ord rank l = Some h -> ord h < ord e ->
      head ord rank (e :: l) = Some h.
  Proof.
    intros E ord rank e l h Hnd Hh Hlt.
    destruct (head_spec E ord rank l h Hh) as [Hin Hmin].
    apply head_unique.
    - apply nodup_keys_unique. exact Hnd.
    - right. exact Hin.
    - intros x [<- | Hx]; [unfold key_le; left; exact Hlt | exact (Hmin x Hx)].
  Qed.

  (* An ord below the head is below every entry of the side. *)
  Lemma head_least_below :
    forall (E : Type) (ord rank : E -> nat) l h o,
      head ord rank l = Some h -> o < ord h -> forall x, In x l -> o < ord x.
  Proof.
    intros E ord rank l h o Hh Hlt x Hx.
    destruct (head_spec E ord rank l h Hh) as [_ Hmin].
    specialize (Hmin x Hx). unfold key_le in Hmin. lia.
  Qed.

  Lemma in_remove_at_neq :
    forall (A : Type) i (l : list A) x y,
      nth_error l i = Some x -> In y l -> y <> x -> In y (remove_at i l).
  Proof.
    intros A i l x y Hn Hy Hne.
    apply (Permutation_in y (remove_at_perm A i l x Hn)) in Hy.
    destruct Hy as [Heq | Hy]; [exfalso; apply Hne; symmetry; exact Heq | exact Hy].
  Qed.

  Lemma nodup_cons_distinct :
    forall (E : Type) (f : E -> nat) e l,
      NoDup (map f l) -> (forall y, In y l -> f e <> f y) -> NoDup (map f (e :: l)).
  Proof.
    intros E f e l Hnd Hne. cbn [map]. apply NoDup_cons_iff. split; [| exact Hnd].
    intros Hin. apply in_map_iff in Hin as [y [Hy Hin]]. exact (Hne y Hin (eq_sym Hy)).
  Qed.

  (* On an equation with a loop of fuel fire_round_cap, the injection
     tactic normalizes the loop and exhausts the memory. The proofs use
     this lemma on such equations. *)
  Lemma some_injective : forall (A : Type) (a b : A), Some a = Some b -> a = b.
  Proof. intros A a b H. injection H as H. exact H. Qed.

  Lemma triple_injective :
    forall (A B C : Type) (a a' : A) (b b' : B) (c c' : C),
      (a, b, c) = (a', b', c') -> a = a' /\ b = b' /\ c = c'.
  Proof.
    intros A B C a a' b b' c c' H. injection H as H1 H2 H3.
    split; [exact H1 | split; [exact H2 | exact H3]].
  Qed.

  (* The shape of a re-entered plan: the loop from the head, or the store
     or the park at its place. *)
  Lemma reenter_offer_shape :
    forall st r o, reenter_offer st r = Some o ->
      o = offer_loop fire_round_cap 0 st r \/ o = mkOutcome [SStore r] (store_record st r) None.
  Proof.
    intros st r o H. unfold reenter_offer in H.
    destruct (reentry_at_head (r_ord r) (option_map r_ord (record_head (q_records st)))) as [[] |];
      [left | right | discriminate H]; apply some_injective in H; symmetry; exact H.
  Qed.

  Lemma reenter_receive_shape :
    forall st k o, reenter_receive st k = Some o ->
      o = receive_loop fire_round_cap 0 st k \/ o = mkOutcome [SPark k] (park_waiter st k) None.
  Proof.
    intros st k o H. unfold reenter_receive in H.
    destruct (reentry_at_head (w_ord k) (option_map w_ord (waiter_head (q_waiters st)))) as [[] |];
      [left | right | discriminate H]; apply some_injective in H; symmetry; exact H.
  Qed.

  (* A re-entered element whose ord no stored entry of its side has is
     never Inconsistent. *)
  Lemma reenter_offer_some :
    forall st r, (forall y, In y (q_records st) -> r_ord r <> r_ord y) ->
      exists o, reenter_offer st r = Some o.
  Proof.
    intros st r Hne. unfold reenter_offer, reentry_at_head.
    destruct (record_head (q_records st)) as [h |] eqn:Eh; cbn [option_map].
    - destruct (head_spec record r_ord r_rank _ _ Eh) as [Hin _].
      rewrite (proj2 (Nat.eqb_neq (r_ord r) (r_ord h)) (Hne h Hin)).
      destruct (ord_precedes (r_ord r) (r_ord h)); eexists; reflexivity.
    - eexists. reflexivity.
  Qed.

  Lemma reenter_receive_some :
    forall st k, (forall y, In y (q_waiters st) -> w_ord k <> w_ord y) ->
      exists o, reenter_receive st k = Some o.
  Proof.
    intros st k Hne. unfold reenter_receive, reentry_at_head.
    destruct (waiter_head (q_waiters st)) as [h |] eqn:Eh; cbn [option_map].
    - destruct (head_spec waiter w_ord w_rank _ _ Eh) as [Hin _].
      rewrite (proj2 (Nat.eqb_neq (w_ord k) (w_ord h)) (Hne h Hin)).
      destruct (ord_precedes (w_ord k) (w_ord h)); eexists; reflexivity.
    - eexists. reflexivity.
  Qed.

  (* The state facts of a re-entered datum plan. *)
  Lemma reenter_offer_facts :
    forall st r o,
      NoDup (map r_ord (q_records st)) -> NoDup (map w_ord (q_waiters st)) ->
      (forall y, In y (q_records st) -> r_ord r <> r_ord y) ->
      reenter_offer st r = Some o ->
      q_counter (o_state o) = q_counter st /\
      NoDup (map r_ord (q_records (o_state o))) /\ NoDup (map w_ord (q_waiters (o_state o))) /\
      (forall y, In y (q_records (o_state o)) -> In y (q_records st) \/ y = r) /\
      (forall w, In w (q_waiters (o_state o)) -> In w (q_waiters st)) /\
      (o_reissue o <> None -> q_records (o_state o) = q_records st) /\
      removed_of o = [] /\
      (forall s, In s (o_ops o) -> stages (FlightRecord r) s).
  Proof.
    intros st r o Hndr Hndw Hne Ho.
    pose proof (nodup_cons_distinct record r_ord r (q_records st) Hndr Hne) as Hcons.
    destruct (reenter_offer_shape st r o Ho) as [-> | ->].
    - pose proof (offer_loop_end fire_round_cap 0 st r) as [Hc [Hw Hend]].
      split; [exact Hc |]. split; [| split; [apply offer_loop_waiters_nodup; exact Hndw |]].
      { destruct Hend as [[Hr _] | [[Hr _] | [Hr _]]]; rewrite Hr; [exact Hcons | exact Hndr | exact Hndr]. }
      split; [| split; [exact Hw | split; [| split; [apply offer_loop_removed | apply offer_loop_stages]]]].
      + intros y Hy. destruct Hend as [[Hr _] | [[Hr _] | [Hr _]]]; rewrite Hr in Hy.
        * destruct Hy as [<- | Hy]; [right; reflexivity | left; exact Hy].
        * left. exact Hy.
        * left. exact Hy.
      + intros Hre. destruct Hend as [[_ [Hn _]] | [[Hr _] | [Hr _]]];
          [exfalso; exact (Hre Hn) | exact Hr | exact Hr].
    - unfold store_record. cbn [o_state o_ops o_reissue q_records q_waiters q_counter].
      split; [reflexivity | split; [exact Hcons | split; [exact Hndw |]]].
      split; [intros y [<- | Hy]; [right; reflexivity | left; exact Hy] |].
      split; [intros w0 H0; exact H0 |].
      split; [intros Hre; exfalso; exact (Hre eq_refl) |].
      split; [reflexivity |].
      intros s [<- | []]. reflexivity.
  Qed.

  (* The state facts of a re-entered continuation plan. *)
  Lemma reenter_receive_facts :
    forall st k o,
      NoDup (map r_ord (q_records st)) -> NoDup (map w_ord (q_waiters st)) ->
      (forall y, In y (q_waiters st) -> w_ord k <> w_ord y) ->
      reenter_receive st k = Some o ->
      q_counter (o_state o) = q_counter st /\
      NoDup (map r_ord (q_records (o_state o))) /\ NoDup (map w_ord (q_waiters (o_state o))) /\
      (forall y, In y (q_records (o_state o)) -> In y (q_records st)) /\
      (forall w, In w (q_waiters (o_state o)) -> In w (q_waiters st) \/ w = k) /\
      (o_reissue o <> None -> q_waiters (o_state o) = q_waiters st) /\
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall y, In y (removed_of o) -> In y (q_records st)) /\
      (forall y z, In y (removed_of o) -> In z (q_records (o_state o)) -> r_ord y < r_ord z) /\
      (forall s, In s (o_ops o) -> stages (FlightWaiter k) s).
  Proof.
    intros st k o Hndr Hndw Hne Ho.
    pose proof (nodup_cons_distinct waiter w_ord k (q_waiters st) Hndw Hne) as Hcons.
    destruct (reenter_receive_shape st k o Ho) as [-> | ->].
    - pose proof (receive_loop_end fire_round_cap 0 st k) as [Hc [Hr Hend]].
      destruct (receive_loop_removed_any fire_round_cap 0 st k Hndr) as [Hs [Hfrom [Hold Hndr']]].
      split; [exact Hc | split; [exact Hndr' |]].
      split; [destruct Hend as [[Hw _] | [[Hw _] | [Hw _]]]; rewrite Hw; [exact Hcons | exact Hndw | exact Hndw] |].
      split; [exact Hr |].
      split.
      { intros y Hy. destruct Hend as [[Hw _] | [[Hw _] | [Hw _]]]; rewrite Hw in Hy.
        - destruct Hy as [<- | Hy]; [right; reflexivity | left; exact Hy].
        - left. exact Hy.
        - left. exact Hy. }
      split.
      { intros Hre. destruct Hend as [[_ [Hn _]] | [[Hw _] | [Hw _]]];
          [exfalso; exact (Hre Hn) | exact Hw | exact Hw]. }
      split; [exact Hs | split; [exact Hfrom | split; [exact Hold | apply receive_loop_stages]]].
    - unfold park_waiter, removed_of. cbn [o_state o_ops o_reissue q_records q_waiters q_counter flat_map app].
      split; [reflexivity | split; [exact Hndr | split; [exact Hcons |]]].
      split; [intros y Hy; exact Hy |].
      split; [intros y [<- | Hy]; [right; reflexivity | left; exact Hy] |].
      split; [intros Hre; exfalso; exact (Hre eq_refl) |].
      split; [constructor | split; [intros y [] | split; [intros y z [] |]]].
      intros s [<- | []]. reflexivity.
  Qed.

  (* ── 17. Decision 43: well-formedness with elements in flight ──────── *)

  (* wf' after a re-entry round: the stored entries are old entries or the
     element f itself. F loses f, and a re-issue puts f back with its ord. *)
  Lemma wf'_after_reentry :
    forall st F i f st' F',
      wf' st F -> nth_error F i = Some f ->
      NoDup (map r_ord (q_records st')) -> NoDup (map w_ord (q_waiters st')) ->
      bump_ord (q_counter st') = S (bump_ord (q_counter st)) ->
      (forall y, In y (q_records st') -> In y (q_records st) \/ f = FlightRecord y) ->
      (forall y, In y (q_waiters st') -> In y (q_waiters st) \/ f = FlightWaiter y) ->
      (F' = remove_at i F \/
       (F' = f :: remove_at i F /\
        (forall y, In y (q_records st') -> In y (q_records st)) /\
        (forall y, In y (q_waiters st') -> In y (q_waiters st)))) ->
      wf' st' F'.
  Proof.
    intros st F i f st' F' [[_ [_ [Hbr Hbw]]] [HndF [HF [HFr HFw]]]] Hn Hndr' Hndw' Hc Hrec Hwait HF'.
    assert (Hin : In f F) by (apply nth_error_In with i; exact Hn).
    assert (Hother : forall g, In g (remove_at i F) -> flight_ord g <> flight_ord f)
      by (intros g Hg; exact (remove_at_nodup_other in_flight flight_ord i F f g Hn HndF Hg)).
    assert (HinF : forall g, In g F' -> In g F).
    { intros g Hg. destruct HF' as [-> | [-> _]].
      - exact (remove_at_in in_flight i F g Hg).
      - destruct Hg as [<- | Hg]; [exact Hin | exact (remove_at_in in_flight i F g Hg)]. }
    destruct (HF f Hin) as [_ Hflt].
    split; [split; [exact Hndr' | split; [exact Hndw' | split]] | split; [| split; [| split]]].
    - intros y Hy. rewrite Hc. destruct (Hrec y Hy) as [Hy0 | Hy0].
      + specialize (Hbr y Hy0). lia.
      + rewrite Hy0 in Hflt. cbn [flight_ord] in Hflt. lia.
    - intros y Hy. rewrite Hc. destruct (Hwait y Hy) as [Hy0 | Hy0].
      + specialize (Hbw y Hy0). lia.
      + rewrite Hy0 in Hflt. cbn [flight_ord] in Hflt. lia.
    - destruct HF' as [-> | [-> _]].
      + apply remove_at_nodup. exact HndF.
      + exact (Permutation_NoDup (Permutation_map flight_ord (remove_at_perm in_flight i F f Hn)) HndF).
    - intros g Hg. destruct (HF g (HinF g Hg)) as [Hp Hlt]. split; [exact Hp | rewrite Hc; lia].
    - intros g y Hg Hy. destruct HF' as [-> | [-> [Hsubr _]]].
      + destruct (Hrec y Hy) as [Hy0 | Hy0].
        * exact (HFr g y (remove_at_in in_flight i F g Hg) Hy0).
        * specialize (Hother g Hg). rewrite Hy0 in Hother. cbn [flight_ord] in Hother. exact Hother.
      + exact (HFr g y (HinF g Hg) (Hsubr y Hy)).
    - intros g y Hg Hy. destruct HF' as [-> | [-> [_ Hsubw]]].
      + destruct (Hwait y Hy) as [Hy0 | Hy0].
        * exact (HFw g y (remove_at_in in_flight i F g Hg) Hy0).
        * specialize (Hother g Hg). rewrite Hy0 in Hother. cbn [flight_ord] in Hother. exact Hother.
      + exact (HFw g y (HinF g Hg) (Hsubw y Hy)).
  Qed.

  (* wf' after a fresh round: every new stored entry has the ord of the
     bump, which no element in flight has. *)
  Lemma wf'_after_fresh :
    forall st F st',
      wf' st F -> wf st' -> bump_ord (q_counter st') = S (bump_ord (q_counter st)) ->
      (forall y, In y (q_records st') -> In y (q_records st) \/ r_ord y = bump_ord (q_counter st)) ->
      (forall y, In y (q_waiters st') -> In y (q_waiters st) \/ w_ord y = bump_ord (q_counter st)) ->
      wf' st' F.
  Proof.
    intros st F st' [_ [HndF [HF [HFr HFw]]]] Hwf' Hc Hrec Hwait.
    split; [exact Hwf' | split; [exact HndF | split; [| split]]].
    - intros f Hf. destruct (HF f Hf) as [Hp Hlt]. split; [exact Hp | rewrite Hc; lia].
    - intros f y Hf Hy. destruct (Hrec y Hy) as [Hy0 | Hy0].
      + exact (HFr f y Hf Hy0).
      + destruct (HF f Hf) as [_ Hlt]. lia.
    - intros f y Hf Hy. destruct (Hwait y Hy) as [Hy0 | Hy0].
      + exact (HFw f y Hf Hy0).
      + destruct (HF f Hf) as [_ Hlt]. lia.
  Qed.

  (* wf' after a fresh round that puts its persistent element x in
     flight. x has the ord of the bump, and the round stores no new
     entry. *)
  Lemma wf'_after_fresh_reissue :
    forall st F st' x,
      wf' st F -> wf st' -> bump_ord (q_counter st') = S (bump_ord (q_counter st)) ->
      flight_pers x = true -> flight_ord x = bump_ord (q_counter st) ->
      (forall y, In y (q_records st') -> In y (q_records st)) ->
      (forall y, In y (q_waiters st') -> In y (q_waiters st)) ->
      wf' st' (x :: F).
  Proof.
    intros st F st' x Hwf Hwf' Hc Hp Ho Hrec Hwait.
    pose proof Hwf as [[_ [_ [Hbr Hbw]]] [HndF [HF _]]].
    destruct (wf'_after_fresh st F st' Hwf Hwf' Hc
                (fun y Hy => or_introl (Hrec y Hy)) (fun y Hy => or_introl (Hwait y Hy)))
      as [_ [_ [HF1 [HFr1 HFw1]]]].
    split; [exact Hwf' | split; [| split; [| split]]].
    - cbn [map]. apply NoDup_cons_iff. split; [| exact HndF].
      intros Hin. apply in_map_iff in Hin as [g [Hg Hin]]. destruct (HF g Hin) as [_ Hlt]. lia.
    - intros g [<- | Hg]; [split; [exact Hp | rewrite Hc; lia] | exact (HF1 g Hg)].
    - intros g y [<- | Hg] Hy; [| exact (HFr1 g y Hg Hy)].
      specialize (Hbr y (Hrec y Hy)). lia.
    - intros g y [<- | Hg] Hy; [| exact (HFw1 g y Hg Hy)].
      specialize (Hbw y (Hwait y Hy)). lia.
  Qed.

  (* ── 18. Decision 43: headlines 10 and 11 (L2 and the kept ord) ────── *)

  (* Headline 10 (L2 of design 43.4). A re-entered persistent element is
     never spent. Its round ends with the element in flight again (a
     re-issue), or with the element stored or parked. A fire spends only a
     non-persistent datum or a non-persistent continuation. *)
  Theorem persistent_reentry_never_spent :
    (forall st r o, r_pers r = true -> reenter_offer st r = Some o ->
       o_reissue o <> None \/ (o_reissue o = None /\ In r (q_records (o_state o)))) /\
    (forall st k o, w_pers k = true -> reenter_receive st k = Some o ->
       o_reissue o <> None \/ (o_reissue o = None /\ In k (q_waiters (o_state o)))).
  Proof.
    split.
    - intros st r o Hp Ho. destruct (reenter_offer_shape st r o Ho) as [-> | ->].
      + pose proof (offer_loop_end fire_round_cap 0 st r) as [_ [_ Hend]].
        destruct Hend as [[Hr [Hn _]] | [[_ [_ Hf]] | [_ Hre]]].
        * right. split; [exact Hn | rewrite Hr; left; reflexivity].
        * rewrite Hp in Hf. discriminate.
        * left. exact Hre.
      + right. cbn [o_reissue o_state store_record q_records]. split; [reflexivity | left; reflexivity].
    - intros st k o Hp Ho. destruct (reenter_receive_shape st k o Ho) as [-> | ->].
      + pose proof (receive_loop_end fire_round_cap 0 st k) as [_ [_ Hend]].
        destruct Hend as [[Hw [Hn _]] | [[_ [_ Hf]] | [_ Hre]]].
        * right. split; [exact Hn | rewrite Hw; left; reflexivity].
        * rewrite Hp in Hf. discriminate.
        * left. exact Hre.
      + right. cbn [o_reissue o_state park_waiter q_waiters]. split; [reflexivity | left; reflexivity].
  Qed.

  (* Headline 11 (headline 7 for re-issued operations). A fresh operation
     takes the bump value as its ord. A re-issued persistent operation
     passes the check of operation_ord and keeps its first-round ord. The
     round plans the same record or continuation, and every staged
     operation of the round names it. So the element keeps its record
     identity. The round never meets the Inconsistent case of at_head. It
     advances the counter by one. It keeps wf' with F without the element,
     or with the element in flight again after a re-issue. *)
  Theorem queue_reissue_keeps_ord :
    (forall bump, operation_ord None bump = Some bump) /\
    (forall st F i f, wf' st F -> nth_error F i = Some f ->
       operation_ord (Some (flight_ord f)) (bump_ord (q_counter st)) = Some (flight_ord f) /\
       reenter st f = reenter_plan (bumped st) f /\
       exists o, reenter st f = Some o /\
         (forall s, In s (o_ops o) -> stages f s) /\
         q_counter (o_state o) = Some (S (bump_ord (q_counter st))) /\
         wf' (o_state o) (match o_reissue o with
                          | Some _ => f :: remove_at i F
                          | None => remove_at i F
                          end)).
  Proof.
    split; [reflexivity |].
    intros st F i f Hwf Hn.
    pose proof Hwf as [[Hndr [Hndw _]] [_ [HF [HFr HFw]]]].
    assert (Hin : In f F) by (apply nth_error_In with i; exact Hn).
    destruct (HF f Hin) as [_ Hlt].
    assert (Hop : operation_ord (Some (flight_ord f)) (bump_ord (q_counter st)) = Some (flight_ord f))
      by (unfold operation_ord; rewrite (proj2 (Nat.ltb_lt _ _) Hlt); reflexivity).
    assert (Hplan : reenter st f = reenter_plan (bumped st) f).
    { destruct f as [[ro rk rv rp] | [wo wk wp wpe wpk]].
      - unfold reenter, reenter_produce, reenter_plan. cbn [flight_ord r_ord r_rank r_val r_pers] in Hop |- *.
        rewrite Hop. reflexivity.
      - unfold reenter, reenter_consume, reenter_plan. cbn [flight_ord w_ord w_rank w_pat w_pers w_peek] in Hop |- *.
        rewrite Hop. reflexivity. }
    split; [exact Hop | split; [exact Hplan |]].
    rewrite Hplan.
    destruct f as [r | k]; cbn [reenter_plan].
    - (* a re-entered datum *)
      assert (Hne : forall y, In y (q_records (bumped st)) -> r_ord r <> r_ord y)
        by (intros y Hy; exact (HFr (FlightRecord r) y Hin Hy)).
      destruct (reenter_offer_some (bumped st) r Hne) as [o Ho].
      destruct (reenter_offer_facts (bumped st) r o Hndr Hndw Hne Ho)
        as [Hc [Hndr' [Hndw' [Hrec [Hwait [Hre [_ Hstages]]]]]]].
      exists o. split; [exact Ho | split; [exact Hstages | split; [rewrite Hc; reflexivity |]]].
      apply (wf'_after_reentry st F i (FlightRecord r) (o_state o));
        [exact Hwf | exact Hn | exact Hndr' | exact Hndw' | rewrite Hc; exact (bump_ord_next (q_counter st)) | | |].
      + intros y Hy. destruct (Hrec y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity].
      + intros y Hy. left. exact (Hwait y Hy).
      + destruct (o_reissue o) as [c |] eqn:Er; [right | left; reflexivity].
        split; [reflexivity | split; [| exact Hwait]].
        intros y Hy. rewrite (Hre ltac:(discriminate)) in Hy. exact Hy.
    - (* a re-entered continuation *)
      assert (Hne : forall y, In y (q_waiters (bumped st)) -> w_ord k <> w_ord y)
        by (intros y Hy; exact (HFw (FlightWaiter k) y Hin Hy)).
      destruct (reenter_receive_some (bumped st) k Hne) as [o Ho].
      destruct (reenter_receive_facts (bumped st) k o Hndr Hndw Hne Ho)
        as [Hc [Hndr' [Hndw' [Hrec [Hwait [Hre [_ [_ [_ Hstages]]]]]]]]].
      exists o. split; [exact Ho | split; [exact Hstages | split; [rewrite Hc; reflexivity |]]].
      apply (wf'_after_reentry st F i (FlightWaiter k) (o_state o));
        [exact Hwf | exact Hn | exact Hndr' | exact Hndw' | rewrite Hc; exact (bump_ord_next (q_counter st)) | | |].
      + intros y Hy. left. exact (Hrec y Hy).
      + intros y Hy. destruct (Hwait y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity].
      + destruct (o_reissue o) as [c |] eqn:Er; [right | left; reflexivity].
        split; [reflexivity | split; [exact Hrec |]].
        intros y Hy. rewrite (Hre ltac:(discriminate)) in Hy. exact Hy.
  Qed.

  (* ── 19. Decision 43: SHADOW ───────────────────────────────────────── *)

  Lemma shadow_nil_iff : forall st, SHADOW st [] <-> no_pending_match st.
  Proof.
    intros st. unfold SHADOW, no_pending_match.
    destruct (record_head (q_records st)) as [h |]; [| split; intros _; exact I].
    destruct (waiter_head (q_waiters st)) as [w |]; [| split; intros _; exact I].
    split.
    - intros H. destruct (fits (w_pat w) (r_val h)) eqn:E; [| reflexivity].
      destruct (H eq_refl) as [f [[] _]].
    - intros H Hfit. rewrite H in Hfit. discriminate.
  Qed.

  Lemma no_pending_match_shadow : forall st F, no_pending_match st -> SHADOW st F.
  Proof.
    intros st F H. unfold no_pending_match, SHADOW in *.
    destruct (record_head (q_records st)) as [h |]; [| exact I].
    destruct (waiter_head (q_waiters st)) as [w |]; [| exact I].
    intros Hfit. rewrite H in Hfit. discriminate.
  Qed.

  (* L4 (design 43.4): with the same heads, a shadow stays valid while its
     element stays in flight. *)
  Lemma shadow_same_heads :
    forall st st' F F',
      record_head (q_records st') = record_head (q_records st) ->
      waiter_head (q_waiters st') = waiter_head (q_waiters st) ->
      (forall f h w, record_head (q_records st) = Some h -> waiter_head (q_waiters st) = Some w ->
         In f F -> shadows f h w -> In f F') ->
      SHADOW st F -> SHADOW st' F'.
  Proof.
    intros st st' F F' Hr Hw Hsub. unfold SHADOW. rewrite Hr, Hw.
    destruct (record_head (q_records st)) as [h |] eqn:Eh; [| intros _; exact I].
    destruct (waiter_head (q_waiters st)) as [w |] eqn:Ew; [| intros _; exact I].
    intros Hsh Hfit. destruct (Hsh Hfit) as [f [Hin Hs]].
    exists f. split; [exact (Hsub f h w eq_refl eq_refl Hin Hs) | exact Hs].
  Qed.

  (* The facts of a fresh produce that SHADOW uses. L3 (design 43.4): when
     the records are not empty, the datum joins their tail and the heads
     stay. Otherwise the loop ends with no pending match, as in headline 2.
     A re-issue stores no record. *)
  Lemma produce_round_facts :
    forall st rank v pers, wf st ->
      let x := mkRecord (bump_ord (q_counter st)) rank v pers in
      let o := produce st rank v pers in
      wf (o_state o) /\ bump_ord (q_counter (o_state o)) = S (bump_ord (q_counter st)) /\
      (forall y, In y (q_records (o_state o)) -> In y (q_records st) \/ y = x) /\
      (forall w, In w (q_waiters (o_state o)) -> In w (q_waiters st)) /\
      (o_reissue o <> None -> q_records (o_state o) = q_records st /\ no_pending_match (o_state o)) /\
      ((record_head (q_records (o_state o)) = record_head (q_records st) /\
        q_waiters (o_state o) = q_waiters st) \/ no_pending_match (o_state o)).
  Proof.
    intros st rank v pers Hwf. cbv zeta.
    destruct (produce_wf st rank v pers Hwf) as [Hwf1 [Hc [Hrec _]]].
    pose proof Hwf as [Hndr [_ [Hbr _]]].
    split; [exact Hwf1 | split; [rewrite Hc; apply bump_ord_next | split; [exact Hrec |]]].
    unfold produce, offer, bumped. cbn [q_records].
    destruct (q_records st) as [| x0 xs] eqn:Erec.
    - pose proof (offer_loop_state fire_round_cap 0
                    (mkQ [] (q_waiters st) (bump_next (q_counter st)))
                    (mkRecord (bump_ord (q_counter st)) rank v pers) eq_refl) as [Hinv [_ [_ Hw]]].
      pose proof (offer_loop_end fire_round_cap 0
                    (mkQ [] (q_waiters st) (bump_next (q_counter st)))
                    (mkRecord (bump_ord (q_counter st)) rank v pers)) as [_ [_ Hend]].
      split; [exact Hw | split; [| right; exact Hinv]].
      intros Hre. split; [| exact Hinv].
      destruct Hend as [[_ [Hn _]] | [[Hr _] | [Hr _]]]; [exfalso; exact (Hre Hn) | exact Hr | exact Hr].
    - cbn [o_state o_reissue]. unfold store_record. cbn [q_records q_waiters q_counter].
      split; [intros w0 H0; exact H0 | split; [intros H; exfalso; exact (H eq_refl) |]].
      left. split; [| reflexivity].
      unfold record_head. apply head_cons_fresh.
      + apply nodup_keys_unique. first [exact Hndr | rewrite Erec in Hndr; exact Hndr].
      + discriminate.
      + intros y Hy. cbn [r_ord]. apply Hbr. first [exact Hy | rewrite Erec; exact Hy].
  Qed.

  (* The facts of a fresh consume that SHADOW uses, symmetric to
     produce_round_facts. *)
  Lemma consume_round_facts :
    forall st rank pat pers peek, wf st ->
      let x := mkWaiter (bump_ord (q_counter st)) rank pat pers peek in
      let o := consume st rank pat pers peek in
      wf (o_state o) /\ bump_ord (q_counter (o_state o)) = S (bump_ord (q_counter st)) /\
      (forall y, In y (q_records (o_state o)) -> In y (q_records st)) /\
      (forall w, In w (q_waiters (o_state o)) -> In w (q_waiters st) \/ w = x) /\
      (o_reissue o <> None -> q_waiters (o_state o) = q_waiters st /\ no_pending_match (o_state o)) /\
      ((q_records (o_state o) = q_records st /\
        waiter_head (q_waiters (o_state o)) = waiter_head (q_waiters st)) \/ no_pending_match (o_state o)).
  Proof.
    intros st rank pat pers peek Hwf. cbv zeta.
    destruct (consume_wf st rank pat pers peek Hwf) as [Hwf1 [Hc [Hrec _]]].
    pose proof Hwf as [_ [Hndw [_ Hbw]]].
    split; [exact Hwf1 | split; [rewrite Hc; apply bump_ord_next | split; [exact Hrec |]]].
    unfold consume, receive, bumped. cbn [q_waiters].
    destruct (q_waiters st) as [| x0 xs] eqn:Ew.
    - pose proof (receive_loop_state fire_round_cap 0
                    (mkQ (q_records st) [] (bump_next (q_counter st)))
                    (mkWaiter (bump_ord (q_counter st)) rank pat pers peek) eq_refl) as [Hinv [Hw _]].
      pose proof (receive_loop_end fire_round_cap 0
                    (mkQ (q_records st) [] (bump_next (q_counter st)))
                    (mkWaiter (bump_ord (q_counter st)) rank pat pers peek)) as [_ [_ Hend]].
      split; [| split; [| right; exact Hinv]].
      + intros w0 H0. destruct Hw as [Hw | Hw]; rewrite Hw in H0; [destruct H0 |].
        destruct H0 as [<- | []]. right. reflexivity.
      + intros Hre. split; [| exact Hinv].
        destruct Hend as [[_ [Hn _]] | [[Hw' _] | [Hw' _]]]; [exfalso; exact (Hre Hn) | exact Hw' | exact Hw'].
    - cbn [o_state o_reissue]. unfold park_waiter. cbn [q_records q_waiters q_counter].
      split; [intros w0 [<- | H0]; [right; reflexivity | left; first [exact H0 | rewrite Ew; exact H0]] |].
      split; [intros H; exfalso; exact (H eq_refl) |].
      left. split; [reflexivity |].
      unfold waiter_head. apply head_cons_fresh.
      + apply nodup_keys_unique. first [exact Hndw | rewrite Ew in Hndw; exact Hndw].
      + discriminate.
      + intros y Hy. cbn [w_ord]. apply Hbw. first [exact Hy | rewrite Ew; exact Hy].
  Qed.

  (* T1 (d) for a datum at the head. The offer loop runs from r, which is
     older than every stored record. L2 excludes a fire that spends r. So
     r ends stored at the head with no match, or in flight again. In
     flight, r is older than the head of the records, so it shadows any
     exposed pair. The lemma takes the outcome as a variable, because the
     kernel check of SHADOW over the loop itself does not end (see the
     header). *)
  Lemma offer_end_shadow :
    forall st F i r o,
      NoDup (map r_ord (q_records st)) -> r_pers r = true ->
      (forall y, In y (q_records st) -> r_ord r < r_ord y) ->
      offer_end st r o ->
      SHADOW (o_state o) (match o_reissue o with
                          | Some _ => FlightRecord r :: remove_at i F
                          | None => remove_at i F
                          end).
  Proof.
    intros st F i r o Hndr Hpers Hlt [_ [_ Hend]].
    destruct Hend as [[Hr [Hre Hm]] | [[_ [_ Hf]] | [Hr Hre]]].
    - rewrite Hre. apply no_pending_match_shadow.
      unfold no_pending_match, record_head. rewrite Hr.
      rewrite (head_cons_least record r_ord r_rank r (q_records st) Hndr Hlt).
      unfold no_match_head_waiter in Hm.
      destruct (waiter_head (q_waiters (o_state o))) as [w |]; [exact Hm | exact I].
    - rewrite Hpers in Hf. discriminate.
    - destruct (o_reissue o) as [c |]; [| exfalso; exact (Hre eq_refl)].
      unfold SHADOW. rewrite Hr.
      destruct (record_head (q_records st)) as [h |] eqn:Eh; [| exact I].
      destruct (waiter_head (q_waiters (o_state o))) as [w |]; [| exact I].
      intros _. exists (FlightRecord r). split; [left; reflexivity |].
      cbn [shadows]. apply Hlt. exact (proj1 (head_spec record r_ord r_rank _ _ Eh)).
  Qed.

  Lemma receive_end_shadow :
    forall st F i k o,
      NoDup (map w_ord (q_waiters st)) -> w_pers k = true ->
      (forall y, In y (q_waiters st) -> w_ord k < w_ord y) ->
      receive_end st k o ->
      SHADOW (o_state o) (match o_reissue o with
                          | Some _ => FlightWaiter k :: remove_at i F
                          | None => remove_at i F
                          end).
  Proof.
    intros st F i k o Hndw Hpers Hlt [_ [_ Hend]].
    destruct Hend as [[Hw [Hre Hm]] | [[_ [_ Hf]] | [Hw Hre]]].
    - rewrite Hre. apply no_pending_match_shadow.
      unfold no_pending_match. unfold waiter_head at 1. rewrite Hw.
      rewrite (head_cons_least waiter w_ord w_rank k (q_waiters st) Hndw Hlt).
      unfold no_match_head_record in Hm.
      destruct (record_head (q_records (o_state o))) as [h |]; [exact Hm | exact I].
    - rewrite Hpers in Hf. discriminate.
    - destruct (o_reissue o) as [c |]; [| exfalso; exact (Hre eq_refl)].
      unfold SHADOW. rewrite Hw.
      destruct (record_head (q_records (o_state o))) as [h |]; [| exact I].
      destruct (waiter_head (q_waiters st)) as [w |] eqn:Ew; [| exact I].
      intros _. exists (FlightWaiter k). split; [left; reflexivity |].
      cbn [shadows]. apply Hlt. exact (proj1 (head_spec waiter w_ord w_rank _ _ Ew)).
  Qed.

  (* T1 (c) and (d) for a re-entered datum. QD-9 decides the place: at the
     head (an empty side, or an ord below the head), the offer loop runs
     from r. Behind an older head, r is stored at its place, the heads
     stay, and r cannot shadow the old pair, so another element does. *)
  Lemma reenter_offer_shadow :
    forall st F i r o,
      NoDup (map r_ord (q_records st)) -> r_pers r = true ->
      (forall y, In y (q_records st) -> r_ord r <> r_ord y) ->
      nth_error F i = Some (FlightRecord r) -> SHADOW st F ->
      reenter_offer st r = Some o ->
      SHADOW (o_state o) (match o_reissue o with
                          | Some _ => FlightRecord r :: remove_at i F
                          | None => remove_at i F
                          end).
  Proof.
    intros st F i r o Hndr Hpers Hne Hn Hsh Ho.
    pose proof (nodup_cons_distinct record r_ord r (q_records st) Hndr Hne) as Hcons.
    unfold reenter_offer, reentry_at_head, ord_precedes in Ho.
    destruct (record_head (q_records st)) as [h |] eqn:Eh; cbn [option_map] in Ho.
    - destruct (head_spec record r_ord r_rank _ _ Eh) as [Hhin _].
      rewrite (proj2 (Nat.eqb_neq (r_ord r) (r_ord h)) (Hne h Hhin)) in Ho.
      destruct (r_ord r <? r_ord h) eqn:Elt; apply some_injective in Ho.
      + (* at the head: r precedes the head h *)
        apply Nat.ltb_lt in Elt.
        pose proof (offer_loop_end fire_round_cap 0 st r) as Hend. rewrite Ho in Hend.
        exact (offer_end_shadow st F i r o Hndr Hpers
                 (head_least_below record r_ord r_rank _ h (r_ord r) Eh Elt) Hend).
      + (* behind the older head h: r is stored at its place *)
        subst o. apply Nat.ltb_ge in Elt.
        assert (Hlt : r_ord h < r_ord r) by (pose proof (Hne h Hhin); lia).
        cbn [o_reissue o_state].
        apply (shadow_same_heads st (store_record st r) F (remove_at i F)).
        * unfold store_record, record_head. cbn [q_records].
          rewrite (head_cons_behind record r_ord r_rank r (q_records st) h Hcons Eh Hlt).
          symmetry. exact Eh.
        * reflexivity.
        * intros f h' w' Eh' _ Hf Hs. rewrite Eh in Eh'. apply some_injective in Eh'. subst h'.
          apply (in_remove_at_neq in_flight i F (FlightRecord r) f Hn Hf).
          intros ->. cbn [shadows] in Hs. lia.
        * exact Hsh.
    - (* an empty records side: r is at the head *)
      apply some_injective in Ho.
      assert (Hnil : q_records st = []) by (apply (proj1 (head_index_none record r_ord r_rank _));
                                            unfold record_head, head in Eh;
                                            destruct (head_index r_ord r_rank (q_records st)); [discriminate | reflexivity]).
      pose proof (offer_loop_end fire_round_cap 0 st r) as Hend. rewrite Ho in Hend.
      apply (offer_end_shadow st F i r o Hndr Hpers); [| exact Hend].
      intros y Hy. rewrite Hnil in Hy. destruct Hy.
  Qed.

  Lemma reenter_receive_shadow :
    forall st F i k o,
      NoDup (map w_ord (q_waiters st)) -> w_pers k = true ->
      (forall y, In y (q_waiters st) -> w_ord k <> w_ord y) ->
      nth_error F i = Some (FlightWaiter k) -> SHADOW st F ->
      reenter_receive st k = Some o ->
      SHADOW (o_state o) (match o_reissue o with
                          | Some _ => FlightWaiter k :: remove_at i F
                          | None => remove_at i F
                          end).
  Proof.
    intros st F i k o Hndw Hpers Hne Hn Hsh Ho.
    pose proof (nodup_cons_distinct waiter w_ord k (q_waiters st) Hndw Hne) as Hcons.
    unfold reenter_receive, reentry_at_head, ord_precedes in Ho.
    destruct (waiter_head (q_waiters st)) as [h |] eqn:Eh; cbn [option_map] in Ho.
    - destruct (head_spec waiter w_ord w_rank _ _ Eh) as [Hhin _].
      rewrite (proj2 (Nat.eqb_neq (w_ord k) (w_ord h)) (Hne h Hhin)) in Ho.
      destruct (w_ord k <? w_ord h) eqn:Elt; apply some_injective in Ho.
      + (* at the head: k precedes the head h *)
        apply Nat.ltb_lt in Elt.
        pose proof (receive_loop_end fire_round_cap 0 st k) as Hend. rewrite Ho in Hend.
        exact (receive_end_shadow st F i k o Hndw Hpers
                 (head_least_below waiter w_ord w_rank _ h (w_ord k) Eh Elt) Hend).
      + (* behind the older head h: k is parked at its place *)
        subst o. apply Nat.ltb_ge in Elt.
        assert (Hlt : w_ord h < w_ord k) by (pose proof (Hne h Hhin); lia).
        cbn [o_reissue o_state].
        apply (shadow_same_heads st (park_waiter st k) F (remove_at i F)).
        * reflexivity.
        * unfold park_waiter, waiter_head. cbn [q_waiters].
          rewrite (head_cons_behind waiter w_ord w_rank k (q_waiters st) h Hcons Eh Hlt).
          symmetry. exact Eh.
        * intros f h' w' _ Ew' Hf Hs. rewrite Eh in Ew'. apply some_injective in Ew'. subst w'.
          apply (in_remove_at_neq in_flight i F (FlightWaiter k) f Hn Hf).
          intros ->. cbn [shadows] in Hs. lia.
        * exact Hsh.
    - (* an empty waiters side: k is at the head *)
      apply some_injective in Ho.
      assert (Hnil : q_waiters st = []) by (apply (proj1 (head_index_none waiter w_ord w_rank _));
                                            unfold waiter_head, head in Eh;
                                            destruct (head_index w_ord w_rank (q_waiters st)); [discriminate | reflexivity]).
      pose proof (receive_loop_end fire_round_cap 0 st k) as Hend. rewrite Ho in Hend.
      apply (receive_end_shadow st F i k o Hndw Hpers); [| exact Hend].
      intros y Hy. rewrite Hnil in Hy. destruct Hy.
  Qed.

  (* ── 20. Decision 43: headlines 12 and 13 (SHADOW and quiescence) ──── *)

  (* Headline 12 (headlines 2 and 3 with elements in flight, T1 of design
     43.4). Every round keeps wf' and SHADOW. A fresh round keeps them
     with the same F. When it re-issues, it also keeps them with its
     persistent element in flight. The re-entry of the element at index i
     of F keeps them with F without the element. After a re-issue, it
     keeps them with the element in flight again. *)
  Theorem queue_reentry_preserves_shadow_inv :
    forall st F, wf' st F -> SHADOW st F ->
      (forall rank v pers,
         let o := produce st rank v pers in
         let x := FlightRecord (mkRecord (bump_ord (q_counter st)) rank v pers) in
         wf' (o_state o) F /\ SHADOW (o_state o) F /\
         (o_reissue o <> None -> pers = true ->
            wf' (o_state o) (x :: F) /\ SHADOW (o_state o) (x :: F))) /\
      (forall rank pat pers peek,
         let o := consume st rank pat pers peek in
         let x := FlightWaiter (mkWaiter (bump_ord (q_counter st)) rank pat pers peek) in
         wf' (o_state o) F /\ SHADOW (o_state o) F /\
         (o_reissue o <> None -> pers = true ->
            wf' (o_state o) (x :: F) /\ SHADOW (o_state o) (x :: F))) /\
      (forall i f, nth_error F i = Some f ->
         exists o, reenter st f = Some o /\
           wf' (o_state o) (match o_reissue o with Some _ => f :: remove_at i F | None => remove_at i F end) /\
           SHADOW (o_state o) (match o_reissue o with Some _ => f :: remove_at i F | None => remove_at i F end)).
  Proof.
    intros st F Hwf Hsh. pose proof Hwf as [Hw0 [_ [HF [HFr HFw]]]].
    split; [| split].
    - (* T1 (a) and (b) for a datum *)
      intros rank v pers. cbv zeta.
      destruct (produce_round_facts st rank v pers Hw0) as [Hwf1 [Hc [Hrec [Hwait [Hre Hheads]]]]].
      split; [| split].
      + apply (wf'_after_fresh st F _ Hwf Hwf1 Hc).
        * intros y Hy. destruct (Hrec y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity].
        * intros y Hy. left. exact (Hwait y Hy).
      + destruct Hheads as [[Hr Hw] | Hinv]; [| apply no_pending_match_shadow; exact Hinv].
        apply (shadow_same_heads st _ F F); [exact Hr | rewrite Hw; reflexivity | | exact Hsh].
        intros f h w _ _ Hf _. exact Hf.
      + intros Hre1 Hp. destruct (Hre Hre1) as [Hr Hinv].
        split; [| apply no_pending_match_shadow; exact Hinv].
        apply (wf'_after_fresh_reissue st F _ _ Hwf Hwf1 Hc); [exact Hp | reflexivity | | exact Hwait].
        intros y Hy. rewrite Hr in Hy. exact Hy.
    - (* T1 (a) and (b) for a continuation *)
      intros rank pat pers peek. cbv zeta.
      destruct (consume_round_facts st rank pat pers peek Hw0) as [Hwf1 [Hc [Hrec [Hwait [Hre Hheads]]]]].
      split; [| split].
      + apply (wf'_after_fresh st F _ Hwf Hwf1 Hc).
        * intros y Hy. left. exact (Hrec y Hy).
        * intros y Hy. destruct (Hwait y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity].
      + destruct Hheads as [[Hr Hw] | Hinv]; [| apply no_pending_match_shadow; exact Hinv].
        apply (shadow_same_heads st _ F F); [rewrite Hr; reflexivity | exact Hw | | exact Hsh].
        intros f h w _ _ Hf _. exact Hf.
      + intros Hre1 Hp. destruct (Hre Hre1) as [Hw Hinv].
        split; [| apply no_pending_match_shadow; exact Hinv].
        apply (wf'_after_fresh_reissue st F _ _ Hwf Hwf1 Hc); [exact Hp | reflexivity | exact Hrec |].
        intros y Hy. rewrite Hw in Hy. exact Hy.
    - (* T1 (c) and (d): a re-entry *)
      intros i f Hn.
      destruct (proj2 queue_reissue_keeps_ord st F i f Hwf Hn) as [_ [Hplan [o [Ho [_ [_ HwF]]]]]].
      exists o. split; [exact Ho | split; [exact HwF |]].
      assert (Hin : In f F) by (apply nth_error_In with i; exact Hn).
      destruct (HF f Hin) as [Hpers _].
      rewrite Hplan in Ho.
      destruct f as [r | k]; cbn [reenter_plan flight_pers] in Ho, Hpers.
      + apply (reenter_offer_shadow (bumped st) F i r o (proj1 Hw0) Hpers); [| exact Hn | exact Hsh | exact Ho].
        intros y Hy. exact (HFr (FlightRecord r) y Hin Hy).
      + apply (reenter_receive_shadow (bumped st) F i k o (proj1 (proj2 Hw0)) Hpers); [| exact Hn | exact Hsh | exact Ho].
        intros y Hy. exact (HFw (FlightWaiter k) y Hin Hy).
  Qed.

  (* Headline 13 (T2 of design 43.4, quiescence). INV is SHADOW with
     nothing in flight. Take any run of fresh rounds and re-entries, with
     the scope rule of flight_after. Along the run, wf' and SHADOW hold.
     INV holds at every point where no element is in flight. A deploy ends
     only when every re-issue of the deploy completes. So INV holds at
     every deploy boundary. *)
  Theorem queue_quiescent_inv :
    (forall st, SHADOW st [] <-> no_pending_match st) /\
    (forall rounds st F, wf' st F -> SHADOW st F ->
       let '(st', F', _) := run_rounds st F rounds in
       wf' st' F' /\ SHADOW st' F' /\ (F' = [] -> no_pending_match st')).
  Proof.
    split; [exact shadow_nil_iff |].
    induction rounds as [| rd rest IH]; intros st F Hwf Hsh.
    - cbn [run_rounds]. split; [exact Hwf | split; [exact Hsh |]].
      intros ->. apply shadow_nil_iff. exact Hsh.
    - cbn [run_rounds].
      destruct (round_plan st F rd) as [[[x o] F0] |] eqn:Ep.
      2: { specialize (IH st F Hwf Hsh). exact IH. }
      assert (Hnext : wf' (o_state o) (flight_after x o F0) /\ SHADOW (o_state o) (flight_after x o F0)).
      { destruct (queue_reentry_preserves_shadow_inv st F Hwf Hsh) as [Hprod [Hcons Hre]].
        unfold flight_after, reissued_ord.
        destruct rd as [[rank v pers | rank pat pers peek] | i]; cbn [round_plan] in Ep.
        - (* a fresh produce: only a persistent datum enters F *)
          apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
          cbn [step fresh_element flight_pers flight_ord r_pers r_ord].
          destruct (Hprod rank v pers) as [HwF [HsF Hx]].
          destruct (o_reissue (produce st rank v pers)) as [c |] eqn:Er; [| split; [exact HwF | exact HsF]].
          destruct pers; [| split; [exact HwF | exact HsF]].
          apply Hx; [discriminate | reflexivity].
        - (* a fresh consume: only a persistent continuation enters F *)
          apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
          cbn [step fresh_element flight_pers flight_ord w_pers w_ord].
          destruct (Hcons rank pat pers peek) as [HwF [HsF Hx]].
          destruct (o_reissue (consume st rank pat pers peek)) as [c |] eqn:Er; [| split; [exact HwF | exact HsF]].
          destruct pers; [| split; [exact HwF | exact HsF]].
          apply Hx; [discriminate | reflexivity].
        - (* a re-entry: the element is persistent and keeps its ord *)
          destruct (nth_error F i) as [f |] eqn:En; [| discriminate].
          destruct (reenter st f) as [o' |] eqn:Ero; [| discriminate].
          apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
          destruct (Hre i f En) as [o'' [Ho'' [HwF HsF]]].
          rewrite Ero in Ho''. apply some_injective in Ho''. subst o''.
          assert (Hin : In f F) by (apply nth_error_In with i; exact En).
          pose proof Hwf as [_ [_ [HF _]]]. destruct (HF f Hin) as [Hpers _].
          rewrite Hpers.
          destruct (o_reissue o') as [c |]; split; [exact HwF | exact HsF | exact HwF | exact HsF]. }
      destruct Hnext as [Hwf1 Hsh1].
      specialize (IH (o_state o) (flight_after x o F0) Hwf1 Hsh1).
      destruct (run_rounds (o_state o) (flight_after x o F0) rest) as [[st' F'] rem].
      exact IH.
  Qed.

  (* ── 21. Decision 43: headline 14 (FIFO with re-entries) ───────────── *)

  Lemma removed_of_nonpersistent : forall o y, In y (removed_of o) -> r_pers y = false.
  Proof.
    intros [ops st re] y. unfold removed_of. cbn [o_ops].
    induction ops as [| s ops IH]; cbn [flat_map]; [intros [] |].
    intros Hy. apply in_app_or in Hy as [Hy | Hy]; [| exact (IH Hy)].
    destruct s as [x | x | w x | k h]; cbn in Hy; try destruct Hy.
    destruct (r_pers h || w_peek k) eqn:E; [destruct Hy |].
    destruct Hy as [<- | []]. apply orb_false_iff in E as [E _]. exact E.
  Qed.

  Lemma flight_after_in :
    forall x o F g, In g (flight_after x o F) -> g = x \/ In g F.
  Proof.
    intros x o F g H. unfold flight_after in H.
    destruct (o_reissue o); [destruct (reissued_ord x) |].
    - destruct H as [-> | H]; [left; reflexivity | right; exact H].
    - right. exact H.
    - right. exact H.
  Qed.

  (* The facts of one round that the FIFO headline uses. The in-flight list
     after the round follows the scope rule: a re-issue keeps the element
     in flight exactly when the element is persistent. *)
  Lemma round_plan_facts :
    forall st F rd x o F0, wf' st F -> round_plan st F rd = Some (x, o, F0) ->
      wf' (o_state o) (match o_reissue o with
                       | Some _ => if flight_pers x then x :: F0 else F0
                       | None => F0
                       end) /\
      bump_ord (q_counter (o_state o)) = S (bump_ord (q_counter st)) /\
      (forall g, In g F0 -> In g F) /\
      (In x F \/ flight_ord x = bump_ord (q_counter st)) /\
      (forall y, In y (q_records (o_state o)) ->
         In y (q_records st) \/ bump_ord (q_counter st) <= r_ord y \/ In (FlightRecord y) F) /\
      StronglySorted (fun a b => r_ord a < r_ord b) (removed_of o) /\
      (forall y, In y (removed_of o) -> In y (q_records st)) /\
      (forall y z, In y (removed_of o) -> In z (q_records (o_state o)) -> r_ord y < r_ord z).
  Proof.
    intros st F rd x o F0 Hwf Ep. pose proof Hwf as [Hw0 [_ [HF [HFr HFw]]]].
    pose proof Hw0 as [Hndr [Hndw _]].
    destruct rd as [[rank v pers | rank pat pers peek] | i]; cbn [round_plan] in Ep.
    - (* a fresh produce *)
      apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
      cbn [step fresh_element flight_pers flight_ord r_pers r_ord w_pers w_ord].
      destruct (produce_round_facts st rank v pers Hw0) as [Hwf1 [Hc [Hrec [Hwait [Hre _]]]]].
      destruct (produce_wf st rank v pers Hw0) as [_ [_ [_ Hrem]]].
      assert (HwF : wf' (o_state (produce st rank v pers)) F).
      { apply (wf'_after_fresh st F _ Hwf Hwf1 Hc).
        - intros y Hy. destruct (Hrec y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity].
        - intros y Hy. left. exact (Hwait y Hy). }
      split; [| split; [exact Hc | split; [intros g Hg; exact Hg | split; [right; reflexivity | split]]]].
      + destruct (o_reissue (produce st rank v pers)) as [c |] eqn:Er; [| exact HwF].
        destruct pers; [| exact HwF].
        apply (wf'_after_fresh_reissue st F _ _ Hwf Hwf1 Hc); [reflexivity | reflexivity | | exact Hwait].
        intros y Hy. rewrite (proj1 (Hre ltac:(discriminate))) in Hy. exact Hy.
      + intros y Hy. destruct (Hrec y Hy) as [Hy0 | ->]; [left; exact Hy0 |].
        right; left. cbn [r_ord]. lia.
      + rewrite Hrem. split; [constructor | split; [intros y [] | intros y z []]].
    - (* a fresh consume *)
      apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
      cbn [step fresh_element flight_pers flight_ord r_pers r_ord w_pers w_ord].
      destruct (consume_round_facts st rank pat pers peek Hw0) as [Hwf1 [Hc [Hrec [Hwait [Hre _]]]]].
      destruct (consume_wf st rank pat pers peek Hw0) as [_ [_ [_ [Hs [Hfrom Hold]]]]].
      assert (HwF : wf' (o_state (consume st rank pat pers peek)) F).
      { apply (wf'_after_fresh st F _ Hwf Hwf1 Hc).
        - intros y Hy. left. exact (Hrec y Hy).
        - intros y Hy. destruct (Hwait y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; reflexivity]. }
      split; [| split; [exact Hc | split; [intros g Hg; exact Hg | split; [right; reflexivity | split]]]].
      + destruct (o_reissue (consume st rank pat pers peek)) as [c |] eqn:Er; [| exact HwF].
        destruct pers; [| exact HwF].
        apply (wf'_after_fresh_reissue st F _ _ Hwf Hwf1 Hc); [reflexivity | reflexivity | exact Hrec |].
        intros y Hy. rewrite (proj1 (Hre ltac:(discriminate))) in Hy. exact Hy.
      + intros y Hy. left. exact (Hrec y Hy).
      + split; [exact Hs | split; [exact Hfrom | exact Hold]].
    - (* a re-entry *)
      destruct (nth_error F i) as [f |] eqn:En; [| discriminate].
      destruct (reenter st f) as [o' |] eqn:Ero; [| discriminate].
      apply some_injective, triple_injective in Ep as [Ex [Eo EF]]. subst x o F0.
      assert (Hin : In f F) by (apply nth_error_In with i; exact En).
      destruct (HF f Hin) as [Hpers _].
      destruct (proj2 queue_reissue_keeps_ord st F i f Hwf En) as [_ [Hplan [o'' [Ho'' [_ [Hc HwF]]]]]].
      rewrite Ero in Ho''. apply some_injective in Ho''. subst o''.
      rewrite Hpers.
      split; [destruct (o_reissue o'); exact HwF |].
      split; [rewrite Hc; reflexivity |].
      split; [intros g Hg; exact (remove_at_in in_flight i F g Hg) |].
      split; [left; exact Hin |].
      rewrite Hplan in Ero.
      destruct f as [r | k]; cbn [reenter_plan] in Ero.
      + (* a datum: it stores at most itself and removes nothing *)
        assert (Hne : forall y, In y (q_records (bumped st)) -> r_ord r <> r_ord y)
          by (intros y Hy; exact (HFr (FlightRecord r) y Hin Hy)).
        destruct (reenter_offer_facts (bumped st) r o' Hndr Hndw Hne Ero)
          as [_ [_ [_ [Hrec [_ [_ [Hrem _]]]]]]].
        split; [| rewrite Hrem; split; [constructor | split; [intros y [] | intros y z []]]].
        intros y Hy. destruct (Hrec y Hy) as [Hy0 | ->]; [left; exact Hy0 | right; right; exact Hin].
      + (* a continuation: it removes head records, as a fresh one does *)
        assert (Hne : forall y, In y (q_waiters (bumped st)) -> w_ord k <> w_ord y)
          by (intros y Hy; exact (HFw (FlightWaiter k) y Hin Hy)).
        destruct (reenter_receive_facts (bumped st) k o' Hndr Hndw Hne Ero)
          as [_ [_ [_ [Hrec [_ [_ [Hs [Hfrom [Hold _]]]]]]]]].
        split; [intros y Hy; left; exact (Hrec y Hy) |].
        split; [exact Hs | split; [exact Hfrom | exact Hold]].
  Qed.

  (* Headline 14 (headline 4 with re-entries, T3 of design 43.4). Take any
     run of fresh rounds and re-entries from a state with wf'. The records
     leave in strictly increasing ord. Every removed record is older than
     every surviving non-persistent record. Only non-persistent records
     leave. A removed record was in the collection at the start, or it
     arrived during the run with a fresh ord. A record of the final state
     was in the collection at the start, or it arrived with a fresh ord.
     Else it is a persistent element that was in flight at the start and
     re-entered with its first-round ord. A persistent record keeps its
     first-round place (queue_reissue_keeps_ord). It is behind every older
     stored entry and ahead of every entry stored after its first round. *)
  Theorem queue_fifo_with_reentries :
    forall rounds st F, wf' st F ->
      let '(st', F', rem) := run_rounds st F rounds in
      wf' st' F' /\
      StronglySorted (fun a b => r_ord a < r_ord b) rem /\
      (forall x, In x rem -> r_pers x = false) /\
      (forall x y, In x rem -> In y (q_records st') -> r_pers y = false -> r_ord x < r_ord y) /\
      (forall x, In x rem -> In x (q_records st) \/ bump_ord (q_counter st) <= r_ord x) /\
      (forall y, In y (q_records st') ->
         In y (q_records st) \/ bump_ord (q_counter st) <= r_ord y \/ In (FlightRecord y) F) /\
      bump_ord (q_counter st) <= bump_ord (q_counter st').
  Proof.
    induction rounds as [| rd rest IH]; intros st F Hwf.
    - cbn [run_rounds].
      split; [exact Hwf |]. split; [constructor |]. split; [intros x [] |].
      split; [intros x y [] |]. split; [intros x [] |].
      split; [intros y Hy; left; exact Hy | lia].
    - cbn [run_rounds].
      destruct (round_plan st F rd) as [[[x o] F0] |] eqn:Ep.
      2: { specialize (IH st F Hwf). exact IH. }
      destruct (round_plan_facts st F rd x o F0 Hwf Ep)
        as [Hwf1 [Hc1 [HF0 [Hx [Hnew1 [Hs1 [Hfrom1 Hold1]]]]]]].
      (* the scope rule of flight_after *)
      assert (Hwf1' : wf' (o_state o) (flight_after x o F0)).
      { revert Hwf1. unfold flight_after, reissued_ord.
        destruct (o_reissue o); [destruct (flight_pers x) |]; intros H; exact H. }
      pose proof Hwf as [[_ [_ [Hbound _]]] [_ [HF _]]].
      pose proof Hwf1' as [_ [_ [HF1 _]]].
      specialize (IH (o_state o) (flight_after x o F0) Hwf1').
      destruct (run_rounds (o_state o) (flight_after x o F0) rest) as [[st' F'] rem2].
      destruct IH as [Hwf' [Hs2 [Hnp2 [Hold2 [Hfrom2 [Hnew2 Hc2]]]]]].
      split; [exact Hwf' |].
      split.
      { (* the removal order *)
        apply StronglySorted_app; [exact Hs1 | exact Hs2 |].
        intros y z Hy Hz. destruct (Hfrom2 z Hz) as [Hz1 | Hz1].
        - exact (Hold1 y z Hy Hz1).
        - specialize (Hbound y (Hfrom1 y Hy)). lia. }
      split.
      { (* only non-persistent records leave *)
        intros y Hy. apply in_app_or in Hy as [Hy | Hy];
          [exact (removed_of_nonpersistent o y Hy) | exact (Hnp2 y Hy)]. }
      split.
      { (* older than every surviving non-persistent record *)
        intros y z Hy Hz Hnp. apply in_app_or in Hy as [Hy | Hy]; [| exact (Hold2 y z Hy Hz Hnp)].
        destruct (Hnew2 z Hz) as [Hz1 | [Hz1 | Hz1]].
        - exact (Hold1 y z Hy Hz1).
        - specialize (Hbound y (Hfrom1 y Hy)). lia.
        - destruct (HF1 _ Hz1) as [Hp _]. cbn [flight_pers] in Hp. rewrite Hp in Hnp. discriminate. }
      split.
      { (* a removed record was present or arrived with a fresh ord *)
        intros y Hy. apply in_app_or in Hy as [Hy | Hy]; [left; exact (Hfrom1 y Hy) |].
        destruct (Hfrom2 y Hy) as [Hy1 | Hy1]; [| right; lia].
        destruct (Hnew1 y Hy1) as [Hy2 | [Hy2 | Hy2]]; [left; exact Hy2 | right; exact Hy2 |].
        destruct (HF _ Hy2) as [Hp _]. cbn [flight_pers] in Hp.
        rewrite (Hnp2 y Hy) in Hp. discriminate. }
      split.
      { (* the records of the final state *)
        intros z Hz. destruct (Hnew2 z Hz) as [Hz1 | [Hz1 | Hz1]].
        - exact (Hnew1 z Hz1).
        - right; left. lia.
        - destruct (flight_after_in x o F0 _ Hz1) as [Heq | Hz2].
          + rewrite <- Heq in Hx. destruct Hx as [Hx | Hx]; [right; right; exact Hx |].
            right; left. cbn [flight_ord] in Hx. lia.
          + right; right. exact (HF0 _ Hz2). }
      lia.
  Qed.

  (* ── 22. Decision 43: headline 15 (head blocks for a re-entry) ─────── *)

  (* Headline 15 (headline 5 for a re-entered element). An older head holds
     the side of a re-entered element: the element is stored or parked at
     its place and fires nothing. At the head, a head of the other side
     that does not match blocks the element. The first fire of a re-entry
     is on the head of the other side. *)
  Theorem queue_head_blocks_reentry :
    (forall st r h, record_head (q_records st) = Some h -> r_ord h < r_ord r ->
       reenter_offer st r = Some (mkOutcome [SStore r] (store_record st r) None)) /\
    (forall st k h, waiter_head (q_waiters st) = Some h -> w_ord h < w_ord k ->
       reenter_receive st k = Some (mkOutcome [SPark k] (park_waiter st k) None)) /\
    (forall st r w,
       (forall h, record_head (q_records st) = Some h -> r_ord r < r_ord h) ->
       waiter_head (q_waiters st) = Some w -> fits (w_pat w) (r_val r) = false ->
       reenter_offer st r = Some (mkOutcome [SStore r] (store_record st r) None)) /\
    (forall st k h,
       (forall h', waiter_head (q_waiters st) = Some h' -> w_ord k < w_ord h') ->
       record_head (q_records st) = Some h -> fits (w_pat k) (r_val h) = false ->
       reenter_receive st k = Some (mkOutcome [SPark k] (park_waiter st k) None)) /\
    (forall st r o w rest, reenter_offer st r = Some o -> o_ops o = SFireProduce w r :: rest ->
       waiter_head (q_waiters st) = Some w) /\
    (forall st k o h rest, reenter_receive st k = Some o -> o_ops o = SFireConsume k h :: rest ->
       record_head (q_records st) = Some h).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros st r h Eh Hlt. unfold reenter_offer, reentry_at_head, ord_precedes. rewrite Eh.
      cbn [option_map].
      rewrite (proj2 (Nat.eqb_neq (r_ord r) (r_ord h)) ltac:(lia)).
      rewrite (proj2 (Nat.ltb_ge (r_ord r) (r_ord h)) ltac:(lia)). reflexivity.
    - intros st k h Eh Hlt. unfold reenter_receive, reentry_at_head, ord_precedes. rewrite Eh.
      cbn [option_map].
      rewrite (proj2 (Nat.eqb_neq (w_ord k) (w_ord h)) ltac:(lia)).
      rewrite (proj2 (Nat.ltb_ge (w_ord k) (w_ord h)) ltac:(lia)). reflexivity.
    - intros st r w Hat Hw Hf.
      unfold waiter_head, head in Hw.
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w'] |] eqn:Ew; [| discriminate].
      cbn [option_map snd] in Hw. apply some_injective in Hw. subst w'.
      assert (Hloop : reenter_offer st r = Some (offer_loop fire_round_cap 0 st r)).
      { unfold reenter_offer, reentry_at_head, ord_precedes.
        destruct (record_head (q_records st)) as [h |] eqn:Eh; cbn [option_map]; [| reflexivity].
        specialize (Hat h eq_refl).
        rewrite (proj2 (Nat.eqb_neq (r_ord r) (r_ord h)) ltac:(lia)).
        rewrite (proj2 (Nat.ltb_lt (r_ord r) (r_ord h)) Hat). reflexivity. }
      rewrite Hloop, (offer_loop_blocked _ _ st r wi w Ew Hf). reflexivity.
    - intros st k h Hat Hr Hf.
      unfold record_head, head in Hr.
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h'] |] eqn:Er; [| discriminate].
      cbn [option_map snd] in Hr. apply some_injective in Hr. subst h'.
      assert (Hloop : reenter_receive st k = Some (receive_loop fire_round_cap 0 st k)).
      { unfold reenter_receive, reentry_at_head, ord_precedes.
        destruct (waiter_head (q_waiters st)) as [h' |] eqn:Eh; cbn [option_map]; [| reflexivity].
        specialize (Hat h' eq_refl).
        rewrite (proj2 (Nat.eqb_neq (w_ord k) (w_ord h')) ltac:(lia)).
        rewrite (proj2 (Nat.ltb_lt (w_ord k) (w_ord h')) Hat). reflexivity. }
      rewrite Hloop, (receive_loop_blocked _ _ st k hi h Er Hf). reflexivity.
    - intros st r o w rest Ho Hops. destruct (reenter_offer_shape st r o Ho) as [-> | ->].
      + exact (offer_loop_first _ _ st r w rest Hops).
      + discriminate Hops.
    - intros st k o h rest Ho Hops. destruct (reenter_receive_shape st k o Ho) as [-> | ->].
      + exact (receive_loop_first _ _ st k h rest Hops).
      + discriminate Hops.
  Qed.

  (* ── 23. Decision 43: headline 16 (the loop bound of a re-entry) ───── *)

  (* Headline 16 (headline 6 for a re-entered element). A re-entry plan
     fires at most FIRE_ROUND_CAP times, and at most once for each entry of
     the other side. A plan that stops at the cap fired exactly
     FIRE_ROUND_CAP times, and a divergent re-issue needs a persistent
     partner. A re-entered persistent element is never spent
     (persistent_reentry_never_spent). The re-issue carries the
     first-round ord exactly for a persistent element, which stays in
     flight. A non-persistent element leaves F and runs again as a fresh
     operation. *)
  Theorem queue_reentry_loop_bounded_or_reissues :
    (forall st r o, reenter_offer st r = Some o ->
       fires o <= fire_round_cap /\ fires o <= length (q_waiters st) /\
       (o_reissue o = Some ReissueRoundCap -> fires o = fire_round_cap) /\
       (o_reissue o = Some ReissueDivergent ->
          exists w, In (SFireProduce w r) (o_ops o) /\ w_pers w = true /\
                    (r_pers r = true \/ w_peek w = true))) /\
    (forall st k o, reenter_receive st k = Some o ->
       fires o <= fire_round_cap /\ fires o <= length (q_records st) /\
       (o_reissue o = Some ReissueRoundCap -> fires o = fire_round_cap) /\
       (o_reissue o = Some ReissueDivergent ->
          w_pers k = true /\
          exists h, In (SFireConsume k h) (o_ops o) /\ (r_pers h = true \/ w_peek k = true))) /\
    (forall st r o, r_pers r = true -> reenter_offer st r = Some o ->
       o_reissue o <> None \/ (o_reissue o = None /\ In r (q_records (o_state o)))) /\
    (forall st k o, w_pers k = true -> reenter_receive st k = Some o ->
       o_reissue o <> None \/ (o_reissue o = None /\ In k (q_waiters (o_state o)))) /\
    (forall x n, reissued_ord x = Some n <-> flight_pers x = true /\ n = flight_ord x) /\
    (forall x o F, o_reissue o <> None ->
       flight_after x o F = (if flight_pers x then x :: F else F)).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros st r o Ho. destruct (reenter_offer_shape st r o Ho) as [-> | ->].
      + pose proof (offer_loop_fires fire_round_cap 0 st r (Nat.add_0_r _)
                      ltac:(unfold fire_round_cap; lia)) as H.
        cbv zeta in H. rewrite !Nat.add_0_l in H. exact H.
      + unfold fires; cbn. split; [unfold fire_round_cap; lia | split; [lia | split; intros H; discriminate]].
    - intros st k o Ho. destruct (reenter_receive_shape st k o Ho) as [-> | ->].
      + pose proof (receive_loop_fires fire_round_cap 0 st k (Nat.add_0_r _)
                      ltac:(unfold fire_round_cap; lia)) as H.
        cbv zeta in H. rewrite !Nat.add_0_l in H. exact H.
      + unfold fires; cbn. split; [unfold fire_round_cap; lia | split; [lia | split; intros H; discriminate]].
    - exact (proj1 persistent_reentry_never_spent).
    - exact (proj2 persistent_reentry_never_spent).
    - intros x n. unfold reissued_ord. destruct (flight_pers x).
      + split; [intros H; apply some_injective in H; split; [reflexivity | symmetry; exact H] |].
        intros [_ ->]. reflexivity.
      + split; [intros H; discriminate H | intros [H _]; discriminate H].
    - intros x o F Hre. unfold flight_after, reissued_ord.
      destruct (o_reissue o) as [c |]; [| exfalso; exact (Hre eq_refl)].
      destruct (flight_pers x); reflexivity.
  Qed.

  (* ── 24. Decision 43: headline 17 (the list order of a re-entry) ───── *)

  Definition outcome_equiv (o1 o2 : outcome) : Prop :=
    o_ops o1 = o_ops o2 /\ o_reissue o1 = o_reissue o2 /\ q_equiv (o_state o1) (o_state o2).

  Definition option_outcome_equiv (a b : option outcome) : Prop :=
    match a, b with
    | Some o1, Some o2 => outcome_equiv o1 o2
    | None, None => True
    | _, _ => False
    end.

  (* Headline 17 (headline 8 for a re-entered element). The round of a
     re-entry depends on the entries of the collection, not on the order
     of the lists of the staged view. Two views with permuted lists give
     the same Inconsistent result or stage the same operations and reach
     permuted views. *)
  Theorem queue_reentry_permutation_invariant :
    forall st1 st2,
      q_equiv st1 st2 ->
      keys_unique r_ord r_rank (q_records st1) -> keys_unique w_ord w_rank (q_waiters st1) ->
      (forall r, option_outcome_equiv (reenter_produce st1 r) (reenter_produce st2 r)) /\
      (forall k, option_outcome_equiv (reenter_consume st1 k) (reenter_consume st2 k)).
  Proof.
    intros st1 st2 [Hpr [Hpw Hc]] Hkr Hkw.
    assert (Hb : q_equiv (bumped st1) (bumped st2))
      by (unfold bumped, q_equiv; cbn [q_records q_waiters q_counter];
          split; [exact Hpr | split; [exact Hpw | rewrite Hc; reflexivity]]).
    split.
    - intros r. unfold reenter_produce. rewrite Hc.
      destruct (operation_ord (Some (r_ord r)) (bump_ord (q_counter st2))) as [o |]; [| exact I].
      unfold reenter_offer.
      change (q_records (bumped st1)) with (q_records st1).
      change (q_records (bumped st2)) with (q_records st2).
      unfold record_head. rewrite (head_perm record r_ord r_rank _ _ Hpr Hkr).
      match goal with
      | |- context [reentry_at_head ?a ?b] => destruct (reentry_at_head a b) as [[] |]
      end.
      + unfold option_outcome_equiv, outcome_equiv.
        exact (offer_loop_perm fire_round_cap 0 _ _ _ Hb Hkw).
      + unfold option_outcome_equiv, outcome_equiv, store_record, q_equiv.
        cbn [o_ops o_reissue o_state q_records q_waiters q_counter bumped].
        split; [reflexivity | split; [reflexivity |]].
        split; [apply perm_skip; exact Hpr | split; [exact Hpw | first [reflexivity | rewrite Hc; reflexivity]]].
      + exact I.
    - intros k. unfold reenter_consume. rewrite Hc.
      destruct (operation_ord (Some (w_ord k)) (bump_ord (q_counter st2))) as [o |]; [| exact I].
      unfold reenter_receive.
      change (q_waiters (bumped st1)) with (q_waiters st1).
      change (q_waiters (bumped st2)) with (q_waiters st2).
      unfold waiter_head. rewrite (head_perm waiter w_ord w_rank _ _ Hpw Hkw).
      match goal with
      | |- context [reentry_at_head ?a ?b] => destruct (reentry_at_head a b) as [[] |]
      end.
      + unfold option_outcome_equiv, outcome_equiv.
        exact (receive_loop_perm fire_round_cap 0 _ _ _ Hb Hkr).
      + unfold option_outcome_equiv, outcome_equiv, park_waiter, q_equiv.
        cbn [o_ops o_reissue o_state q_records q_waiters q_counter bumped].
        split; [reflexivity | split; [reflexivity |]].
        split; [exact Hpr | split; [apply perm_skip; exact Hpw | first [reflexivity | rewrite Hc; reflexivity]]].
      + exact I.
  Qed.

  (* ── 25. Decision 45: headline 20 (the re-store of a peek) ─────────── *)

  (* Rule E (decision 45). The COMM of a peek fire removes each stored
     non-persistent record that it binds, as the host COMM does. Right
     after the fire, the code stages a produce of the same record bytes
     with outcome Store and persist = false: the re-store. The record keeps
     its bytes, its identity and its ord, so it stays the head. o_ops is
     the logical plan. It keeps the peeked record in o_state, and it has no
     re-store. code_ops is the list of the staged operations of the code,
     and sub_records applies their substrate effects to the records. *)

  (* A staged operation of the code is an operation of the plan or the
     re-store of a peeked record (Cx::restore). *)
  Inductive code_staged : Type :=
  | CStaged : staged -> code_staged
  | CRestore : record -> code_staged.

  (* code_of gives the code operations of one plan operation. The re-store
     of a record comes right after each consume fire that peeks at the
     record, when the record is not persistent. This is
     restore_peeked after Cx::fire_receive, in each FireStep branch of
     receive_head. *)
  Definition code_of (s : staged) : list code_staged :=
    match s with
    | SFireConsume k h =>
        if w_peek k && negb (r_pers h) then [CStaged s; CRestore h] else [CStaged s]
    | _ => [CStaged s]
    end.

  Definition code_ops (ops : list staged) : list code_staged := flat_map code_of ops.

  (* remove_key removes the first record with the key (o, rk). The
     substrate removes the stored record that a fire binds, at the index of
     the record in the hot store. The model names that record by its key
     (ord, rank) instead. A key needs no decidable equality on V. A key also
     names the record in any permutation of the plan view. A view index
     does not, and a re-store moves its record to the front of the
     substrate list. Under wf, no two records share an ord, so the key
     names exactly one entry of the list (remove_key_perm). *)
  Fixpoint remove_key (o rk : nat) (l : list record) : list record :=
    match l with
    | [] => []
    | x :: rest => if (r_ord x =? o) && (r_rank x =? rk) then rest else x :: remove_key o rk rest
    end.

  (* sub_step applies one code operation to the records of the substrate
     view (Rule E). A store and a re-store insert the record at the front
     (put_datum). A park and a produce-side fire change no record. The
     incoming datum of a produce-side fire is never stored, and a queue
     waiter binds no stored record. A consume fire removes the record that
     it binds when the record is not persistent, also when the fire
     peeks. *)
  Definition sub_step (l : list record) (c : code_staged) : list record :=
    match c with
    | CStaged (SStore r) => r :: l
    | CStaged (SPark _) => l
    | CStaged (SFireProduce _ _) => l
    | CStaged (SFireConsume _ h) => if r_pers h then l else remove_key (r_ord h) (r_rank h) l
    | CRestore h => h :: l
    end.

  (* sub_records applies the code operations cs, in their order, to the
     records l of the substrate view. *)
  Definition sub_records (cs : list code_staged) (l : list record) : list record :=
    fold_left sub_step cs l.

  (* retention_step is the substrate before decision 45 (retention): a
     peek fire keeps the records that it binds. Only the negative control
     restore_over_retention_duplicates uses it. *)
  Definition retention_step (l : list record) (c : code_staged) : list record :=
    match c with
    | CStaged (SFireConsume k _) => if w_peek k then l else sub_step l c
    | _ => sub_step l c
    end.

  Definition retention_records (cs : list code_staged) (l : list record) : list record :=
    fold_left retention_step cs l.

  Lemma sub_records_app :
    forall c1 c2 l, sub_records (c1 ++ c2) l = sub_records c2 (sub_records c1 l).
  Proof. intros c1 c2 l. unfold sub_records. apply fold_left_app. Qed.

  Lemma sub_records_code_cons :
    forall s ops l,
      sub_records (code_ops (s :: ops)) l = sub_records (code_ops ops) (sub_records (code_of s) l).
  Proof. intros s ops l. unfold code_ops. cbn [flat_map]. apply sub_records_app. Qed.

  Lemma sub_records_code_one :
    forall s l, sub_records (code_ops [s]) l = sub_records (code_of s) l.
  Proof. intros s l. rewrite sub_records_code_cons. reflexivity. Qed.

  (* With unique keys, the removal by the key of an entry h of a list
     removes h itself. *)
  Lemma remove_key_perm :
    forall l h, In h l -> keys_unique r_ord r_rank l ->
      Permutation l (h :: remove_key (r_ord h) (r_rank h) l).
  Proof.
    induction l as [| x rest IH]; intros h Hin Hk; [destruct Hin |].
    cbn [remove_key].
    destruct ((r_ord x =? r_ord h) && (r_rank x =? r_rank h)) eqn:E.
    - apply andb_true_iff in E as [E1 E2]. apply Nat.eqb_eq in E1. apply Nat.eqb_eq in E2.
      rewrite (Hk x h (or_introl eq_refl) Hin E1 E2). reflexivity.
    - destruct Hin as [-> | Hin].
      + rewrite !Nat.eqb_refl in E. discriminate E.
      + assert (Hk' : keys_unique r_ord r_rank rest)
          by (apply (keys_unique_incl record r_ord r_rank (x :: rest));
              [intros y Hy; right; exact Hy | exact Hk]).
        eapply perm_trans; [apply perm_skip; exact (IH h Hin Hk') | apply perm_swap].
  Qed.

  (* restore_fire_step covers one consume fire. Its code operations are
     the fire, then the re-store after a peek at a non-persistent record.
     They take a permutation of the plan view to a permutation of the plan
     view after the fire. *)
  Lemma restore_fire_step :
    forall st hi h k l,
      NoDup (map r_ord (q_records st)) -> nth_error (q_records st) hi = Some h ->
      Permutation l (q_records st) ->
      Permutation (sub_records (code_of (SFireConsume k h)) l)
        (q_records (if r_pers h || w_peek k then st
                    else mkQ (remove_at hi (q_records st)) (q_waiters st) (q_counter st))).
  Proof.
    intros st hi h k l Hnd Hnth Hp.
    assert (Hin : In h l)
      by (apply (Permutation_in h (Permutation_sym Hp)); exact (nth_error_In _ _ Hnth)).
    assert (Hk : keys_unique r_ord r_rank l).
    { apply (keys_unique_incl record r_ord r_rank (q_records st)); [| apply nodup_keys_unique; exact Hnd].
      intros y Hy. exact (Permutation_in y Hp Hy). }
    pose proof (remove_key_perm l h Hin Hk) as Hl.
    unfold code_of, sub_records.
    destruct (r_pers h) eqn:Ep; destruct (w_peek k) eqn:Ek;
      cbn [andb negb orb fold_left sub_step q_records]; rewrite ?Ep; cbn [andb negb orb].
    - (* a persistent record stays, and nothing is re-stored *)
      exact Hp.
    - (* a persistent record and no peek: the same *)
      exact Hp.
    - (* a peek: the fire removes h, and the re-store puts h back *)
      exact (perm_trans (Permutation_sym Hl) Hp).
    - (* a plain fire: the fire removes h, as the plan does *)
      apply (Permutation_cons_inv (a := h)).
      eapply perm_trans; [exact (Permutation_sym Hl) |].
      eapply perm_trans; [exact Hp | exact (remove_at_perm record hi _ h Hnth)].
  Qed.

  (* receive_loop_restore covers the receive loop from any staged view
     whose ords are distinct. Its code operations, with the Rule E effects,
     take a permutation of the plan view to a permutation of the records of
     its outcome. A peek fire stays, so the loop stops after it. So its
     re-store is the last code operation of the loop. *)
  Lemma receive_loop_restore :
    forall fuel fired st k l,
      NoDup (map r_ord (q_records st)) -> Permutation l (q_records st) ->
      Permutation (sub_records (code_ops (o_ops (receive_loop fuel fired st k))) l)
                  (q_records (o_state (receive_loop fuel fired st k))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k l Hnd Hp;
      unfold receive_loop; fold receive_loop;
      destruct (head_index r_ord r_rank (q_records st)) as [[hi h] |] eqn:Eh.
    2, 4: cbn [o_ops o_state park_waiter q_records]; rewrite sub_records_code_one; exact Hp.
    all: destruct (head_index_head record r_ord r_rank _ _ _ Eh) as [_ Hnth].
    all: destruct (fits (w_pat k) (r_val h)) eqn:Efit; cbn [negb].
    2, 4: cbn [o_ops o_state park_waiter q_records]; rewrite sub_records_code_one; exact Hp.
    all: pose proof (restore_fire_step st hi h k l Hnd Hnth Hp) as Hstep.
    all: destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k)).
    (* fuel S, FContinue: the loop goes on with the next head *)
    6: { cbn [o_ops o_state]. rewrite sub_records_code_cons.
         apply IH; [| exact Hstep].
         destruct (r_pers h || w_peek k); cbn [q_records]; [exact Hnd |].
         apply remove_at_nodup. exact Hnd. }
    all: cbn [o_ops o_state]; rewrite sub_records_code_one; exact Hstep.
  Qed.

  (* offer_loop_restore covers the offer loop from any staged view. A
     produce-side fire changes no record, and a store inserts the datum at
     the front, as the plan does. *)
  Lemma offer_loop_restore :
    forall fuel fired st r l,
      Permutation l (q_records st) ->
      Permutation (sub_records (code_ops (o_ops (offer_loop fuel fired st r))) l)
                  (q_records (o_state (offer_loop fuel fired st r))).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r l Hp;
      unfold offer_loop; fold offer_loop;
      destruct (head_index w_ord w_rank (q_waiters st)) as [[wi w] |] eqn:Eh.
    2, 4: cbn [o_ops o_state store_record q_records]; rewrite sub_records_code_one;
          apply perm_skip; exact Hp.
    all: destruct (fits (w_pat w) (r_val r)) eqn:Efit; cbn [negb].
    2, 4: cbn [o_ops o_state store_record q_records]; rewrite sub_records_code_one;
          apply perm_skip; exact Hp.
    all: assert (Hp1 : Permutation l (q_records (if w_pers w then st
                                                 else mkQ (q_records st) (remove_at wi (q_waiters st)) (q_counter st))))
           by (destruct (w_pers w); exact Hp).
    all: destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w)).
    (* fuel S, FContinue: the loop goes on with the next head *)
    6: { cbn [o_ops o_state]. rewrite sub_records_code_cons. apply IH. exact Hp1. }
    all: cbn [o_ops o_state]; rewrite sub_records_code_one; exact Hp1.
  Qed.

  (* Headline 20 (decision 45, Rule E). Take a well-formed collection and a
     plan: a fresh consume, a fresh produce or a re-entry. Apply the code
     operations of the plan to the records of the view, with the substrate
     effects of Rule E. A consume fire removes the non-persistent record
     that it binds, also on a peek. A store and a re-store insert their
     record at the front. The result holds the records of the logical plan
     (o_state), up to the list order. So the removal and the re-store of a
     peek together keep the peeked record, as the plan does. The list order
     does not change a plan (queue_plan_permutation_invariant). *)
  Theorem queue_restore_realizes_plan :
    forall st, wf st ->
      (forall rank pat pers peek,
         Permutation (sub_records (code_ops (o_ops (consume st rank pat pers peek))) (q_records (bumped st)))
                     (q_records (o_state (consume st rank pat pers peek)))) /\
      (forall rank v pers,
         Permutation (sub_records (code_ops (o_ops (produce st rank v pers))) (q_records (bumped st)))
                     (q_records (o_state (produce st rank v pers)))) /\
      (forall f o, reenter st f = Some o ->
         Permutation (sub_records (code_ops (o_ops o)) (q_records (bumped st))) (q_records (o_state o))).
  Proof.
    intros st [Hndr _].
    split; [| split].
    - (* a fresh consume *)
      intros rank pat pers peek. unfold consume, receive.
      destruct (q_waiters (bumped st)) as [| w ws].
      + apply receive_loop_restore; [exact Hndr | apply Permutation_refl].
      + cbn [o_ops o_state park_waiter q_records]. rewrite sub_records_code_one.
        apply Permutation_refl.
    - (* a fresh produce *)
      intros rank v pers. unfold produce, offer.
      destruct (q_records (bumped st)) as [| x xs] eqn:Er.
      + apply offer_loop_restore. rewrite Er. apply perm_nil.
      + cbn [o_ops o_state store_record q_records]. rewrite sub_records_code_one, Er.
        apply Permutation_refl.
    - (* a re-entry *)
      intros f o Ho. destruct f as [r | k]; cbn [reenter] in Ho.
      + unfold reenter_produce in Ho.
        destruct (operation_ord (Some (r_ord r)) (bump_ord (q_counter st))) as [n |]; [| discriminate Ho].
        destruct (reenter_offer_shape _ _ o Ho) as [-> | ->].
        * apply offer_loop_restore. apply Permutation_refl.
        * cbn [o_ops o_state store_record q_records]. rewrite sub_records_code_one.
          apply Permutation_refl.
      + unfold reenter_consume in Ho.
        destruct (operation_ord (Some (w_ord k)) (bump_ord (q_counter st))) as [n |]; [| discriminate Ho].
        destruct (reenter_receive_shape _ _ o Ho) as [-> | ->].
        * apply receive_loop_restore; [exact Hndr | apply Permutation_refl].
        * cbn [o_ops o_state park_waiter q_records]. rewrite sub_records_code_one.
          apply Permutation_refl.
  Qed.
End Queue.

(* ── 26. Headline 9: the legacy scan breaks head-only ────────────────── *)

(* A pattern matches a value when they are equal. The head waiter w1 (ord
   0) waits for 7. The younger waiter w2 (ord 1) waits for 5. The datum is
   5. *)
Definition example_w1 : waiter nat := mkWaiter nat 0 0 7 false false.
Definition example_w2 : waiter nat := mkWaiter nat 1 0 5 false false.
Definition example_datum : record nat := mkRecord nat 2 0 5 false.
Definition example_state : qstate nat nat := mkQ nat nat [] [example_w1; example_w2] (Some 2).

(* Headline 9. The legacy waiter scan fires the younger waiter w2 when the
   head w1 does not match, so it breaks head-blocks (and the FIFO order of
   the waiters). The head-only plan of P4 stores the datum instead. *)
Theorem legacy_scan_violates_head_blocks :
  waiter_head nat (q_waiters nat nat example_state) = Some example_w1 /\
  Nat.eqb (w_pat nat example_w1) (r_val nat example_datum) = false /\
  o_ops nat nat (offer nat nat Nat.eqb example_state example_datum) = [SStore nat nat example_datum] /\
  o_ops nat nat (legacy_offer nat nat Nat.eqb example_state example_datum) =
    [SFireProduce nat nat example_w2 example_datum] /\
  example_w2 <> example_w1 /\
  w_ord nat example_w1 < w_ord nat example_w2.
Proof.
  split; [reflexivity | split; [reflexivity | split; [reflexivity | split; [reflexivity |]]]].
  split; [discriminate | cbn; lia].
Qed.

(* The premise of headline 8 holds for every well-formed collection, so
   the theorem does not hold vacuously. *)
Corollary plan_permutation_invariant_instance :
  forall (V P : Type) (st : qstate V P),
    wf V P st -> keys_unique (r_ord V) (r_rank V) (q_records V P st) /\
                 keys_unique (w_ord P) (w_rank P) (q_waiters V P st).
Proof.
  intros V P st [Hr [Hw _]]. split; apply nodup_keys_unique; assumption.
Qed.

(* ── 27. Decision 43: headlines 18 and 19 (C0 and C1) ──────────────────── *)

(* The patterns of the examples: None matches every value, and Some n
   matches the value n only. *)
Definition pattern_fits (p : option nat) (v : nat) : bool :=
  match p with None => true | Some n => Nat.eqb n v end.

(* C0 (design 43.2). W (ord 0) is a persistent waiter for 1. The send
   x = q!!(1) takes ord 1, fires W, and both stay, so x re-issues (A10).
   The send y = q!(2) takes ord 2 and is stored, because W does not match
   2. *)
Definition c0_W : waiter (option nat) := mkWaiter (option nat) 0 0 (Some 1) true false.
Definition c0_x : record nat := mkRecord nat 1 0 1 true.
Definition c0_y : record nat := mkRecord nat 2 0 2 false.
Definition c0_x_fresh : record nat := mkRecord nat 3 0 1 true.
Definition c0_st0 : qstate nat (option nat) := mkQ nat (option nat) [] [c0_W] (Some 1).
Definition c0_st1 : qstate nat (option nat) := mkQ nat (option nat) [] [c0_W] (Some 2).
Definition c0_st2 : qstate nat (option nat) := mkQ nat (option nat) [c0_y] [c0_W] (Some 3).

(* Headline 18 (C0). With the fresh ord of P4, the round 2 of x takes ord
   3. This is a new record, and it queues behind the younger y. So x loses
   the FIFO place of its first round. With decision 43, the round 2 of x
   keeps ord 1. The same record re-enters ahead of y and fires W again. *)
Theorem fresh_ord_reissue_loses_first_round_place :
  produce nat (option nat) pattern_fits c0_st0 0 1 true =
    mkOutcome nat (option nat) [SFireProduce nat (option nat) c0_W c0_x] c0_st1 (Some ReissueDivergent) /\
  o_state nat (option nat) (produce nat (option nat) pattern_fits c0_st1 0 2 false) = c0_st2 /\
  r_ord nat c0_x < r_ord nat c0_y /\
  produce nat (option nat) pattern_fits c0_st2 0 1 true =
    mkOutcome nat (option nat) [SStore nat (option nat) c0_x_fresh]
      (mkQ nat (option nat) [c0_x_fresh; c0_y] [c0_W] (Some 4)) None /\
  record_head nat [c0_x_fresh; c0_y] = Some c0_y /\
  r_ord nat c0_y < r_ord nat c0_x_fresh /\ c0_x_fresh <> c0_x /\
  reenter_produce nat (option nat) pattern_fits c0_st2 c0_x =
    Some (mkOutcome nat (option nat) [SFireProduce nat (option nat) c0_W c0_x]
            (mkQ nat (option nat) [c0_y] [c0_W] (Some 4)) (Some ReissueDivergent)).
Proof.
  split; [reflexivity |]. split; [reflexivity |]. split; [cbn; lia |].
  split; [reflexivity |]. split; [reflexivity |]. split; [cbn; lia |].
  split; [discriminate | reflexivity].
Qed.

(* C1 (design 43.2). 64 non-persistent peek waiters for any value (ords 0
   to 63) and W, a waiter for 1 (ord 64), wait on the queue. The deploy
   sends x = q!(1) (ord 65), which fires the 64 peek waiters and re-issues
   at FIRE_ROUND_CAP (I-21). Then y = q!(2) (ord 66) is stored, because W
   does not match 2. Z, a receive for any value (ord 67), parks behind
   W. *)
Definition c1_peeks : list (waiter (option nat)) :=
  map (fun n => mkWaiter (option nat) n 0 None false true) (seq 0 64).
Definition c1_W : waiter (option nat) := mkWaiter (option nat) 64 0 (Some 1) false false.
Definition c1_x : record nat := mkRecord nat 65 0 1 false.
Definition c1_y : record nat := mkRecord nat 66 0 2 false.
Definition c1_Z : waiter (option nat) := mkWaiter (option nat) 67 0 None false false.
Definition c1_x_fresh : record nat := mkRecord nat 68 0 1 false.
Definition c1_st0 : qstate nat (option nat) := mkQ nat (option nat) [] (c1_W :: c1_peeks) (Some 65).
Definition c1_st1 : qstate nat (option nat) := mkQ nat (option nat) [] [c1_W] (Some 66).
Definition c1_st2 : qstate nat (option nat) := mkQ nat (option nat) [c1_y] [c1_W] (Some 67).
Definition c1_st3 : qstate nat (option nat) := mkQ nat (option nat) [c1_y] [c1_Z; c1_W] (Some 68).
Definition c1_good : qstate nat (option nat) :=
  mkQ nat (option nat) [c1_x_fresh; c1_y] [c1_Z; c1_W] (Some 69).
Definition c1_bad : qstate nat (option nat) := mkQ nat (option nat) [c1_y] [c1_Z] (Some 69).

(* The deploy with the scope rule: the re-issue of the non-persistent x is
   a fresh round, so it takes the ord 68. *)
Definition c1_deploy : list (round nat (option nat)) :=
  [RoundFresh nat (option nat) (OpProduce nat (option nat) 0 1 false);
   RoundFresh nat (option nat) (OpProduce nat (option nat) 0 2 false);
   RoundFresh nat (option nat) (OpConsume nat (option nat) 0 None false false);
   RoundFresh nat (option nat) (OpProduce nat (option nat) 0 1 false)].

(* Headline 19 (C1). The deploy starts with no pending match. With the
   scope rule, the deploy ends with nothing in flight and with INV. If the
   non-persistent x kept its first-round ord 65, its round 2 would precede
   y, fire W and spend x. Then nothing is in flight, so the deploy can end,
   but the heads y and Z match: INV fails at the deploy boundary. *)
Theorem nonpersistent_first_ord_reentry_breaks_quiescent_inv :
  no_pending_match nat (option nat) pattern_fits c1_st0 /\
  o_reissue nat (option nat) (produce nat (option nat) pattern_fits c1_st0 0 1 false) =
    Some ReissueRoundCap /\
  o_state nat (option nat) (produce nat (option nat) pattern_fits c1_st0 0 1 false) = c1_st1 /\
  o_state nat (option nat) (produce nat (option nat) pattern_fits c1_st1 0 2 false) = c1_st2 /\
  o_state nat (option nat) (consume nat (option nat) pattern_fits c1_st2 0 None false false) = c1_st3 /\
  run_rounds nat (option nat) pattern_fits c1_st0 [] c1_deploy = (c1_good, [], []) /\
  no_pending_match nat (option nat) pattern_fits c1_good /\
  r_ord nat c1_x < r_ord nat c1_y /\
  reenter_produce nat (option nat) pattern_fits c1_st3 c1_x =
    Some (mkOutcome nat (option nat) [SFireProduce nat (option nat) c1_W c1_x] c1_bad None) /\
  ~ SHADOW nat (option nat) pattern_fits c1_bad [].
Proof.
  split; [exact I |].
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [reflexivity |]. split; [reflexivity |]. split; [cbn; lia |].
  split; [reflexivity |].
  intros H. destruct (H eq_refl) as [f [[] _]].
Qed.

(* ── 28. Decision 45: the negative controls of headline 20 ──────────────── *)

(* A queue holds one non-persistent record h (ord 0) with the value 5, and
   no waiter. A peek receive for any value takes the ord 1. It fires h and
   is spent, so the plan stops, and h stays in the plan view. *)
Definition peek_h : record nat := mkRecord nat 0 0 5 false.
Definition peek_k : waiter (option nat) := mkWaiter (option nat) 1 0 None false true.
Definition peek_st : qstate nat (option nat) := mkQ nat (option nat) [peek_h] [] (Some 1).
Definition peek_plan : outcome nat (option nat) :=
  consume nat (option nat) pattern_fits peek_st 0 None false true.

(* This control puts the re-store on the substrate before decision 45. A
   peek fire kept its record (retention). With the re-store on top of
   retention, the substrate view holds h twice, because the hot store does
   not deduplicate data. So the permutation of headline 20 fails. Rule E
   removes h and stores it again, so the view holds h once. The collection
   is well formed, so headline 20 applies to it. *)
Lemma restore_over_retention_duplicates :
  wf nat (option nat) peek_st /\
  o_ops nat (option nat) peek_plan = [SFireConsume nat (option nat) peek_k peek_h] /\
  q_records nat (option nat) (o_state nat (option nat) peek_plan) = [peek_h] /\
  code_ops nat (option nat) (o_ops nat (option nat) peek_plan) =
    [CStaged nat (option nat) (SFireConsume nat (option nat) peek_k peek_h);
     CRestore nat (option nat) peek_h] /\
  sub_records nat (option nat) (code_ops nat (option nat) (o_ops nat (option nat) peek_plan))
    (q_records nat (option nat) (bumped nat (option nat) peek_st)) = [peek_h] /\
  retention_records nat (option nat) (code_ops nat (option nat) (o_ops nat (option nat) peek_plan))
    (q_records nat (option nat) (bumped nat (option nat) peek_st)) = [peek_h; peek_h] /\
  ~ Permutation
      (retention_records nat (option nat) (code_ops nat (option nat) (o_ops nat (option nat) peek_plan))
         (q_records nat (option nat) (bumped nat (option nat) peek_st)))
      (q_records nat (option nat) (o_state nat (option nat) peek_plan)).
Proof.
  split.
  { unfold wf, peek_st, peek_h. cbn [q_records q_waiters q_counter map bump_ord r_ord].
    split; [apply NoDup_cons; [intros [] | constructor] |].
    split; [constructor |].
    split; [intros r [<- | []]; cbn [r_ord]; lia | intros w []]. }
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [reflexivity |]. split; [reflexivity |].
  intros H. apply Permutation_length in H. discriminate H.
Qed.

(* This control applies the removal of Rule E without the re-store: each
   plan operation is staged alone. The peek fire removes h, and nothing
   stores h again. The substrate view loses h, so the permutation of
   headline 20 fails. *)
Lemma removal_without_restore_loses_record :
  sub_records nat (option nat) (map (CStaged nat (option nat)) (o_ops nat (option nat) peek_plan))
    (q_records nat (option nat) (bumped nat (option nat) peek_st)) = [] /\
  ~ Permutation
      (sub_records nat (option nat) (map (CStaged nat (option nat)) (o_ops nat (option nat) peek_plan))
         (q_records nat (option nat) (bumped nat (option nat) peek_st)))
      (q_records nat (option nat) (o_state nat (option nat) peek_plan)).
Proof.
  split; [reflexivity |].
  intros H. apply Permutation_length in H. discriminate H.
Qed.

(* ── 29. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions queue_head_is_min_ord.
Print Assumptions queue_produce_preserves_inv.
Print Assumptions queue_consume_preserves_inv.
Print Assumptions queue_fifo.
Print Assumptions queue_head_blocks.
Print Assumptions queue_loop_bounded_or_reissues.
Print Assumptions queue_ord_fresh.
Print Assumptions queue_plan_permutation_invariant.
Print Assumptions legacy_scan_violates_head_blocks.
Print Assumptions persistent_reentry_never_spent.
Print Assumptions queue_reissue_keeps_ord.
Print Assumptions queue_reentry_preserves_shadow_inv.
Print Assumptions queue_quiescent_inv.
Print Assumptions queue_fifo_with_reentries.
Print Assumptions queue_head_blocks_reentry.
Print Assumptions queue_reentry_loop_bounded_or_reissues.
Print Assumptions queue_reentry_permutation_invariant.
Print Assumptions fresh_ord_reissue_loses_first_round_place.
Print Assumptions nonpersistent_first_ord_reentry_breaks_quiescent_inv.
Print Assumptions queue_restore_realizes_plan.
