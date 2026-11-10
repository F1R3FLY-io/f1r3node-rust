(* ═══════════════════════════════════════════════════════════════════════════
   CellDiscipline.v — the cell discipline of the native spaces (P5a, F15)

   A cell holds at most one record and at most one waiting continuation
   (F15 of the P5 design, approved by plan decision 39). The cell runs the
   head driver of the engine (offer_head and receive_head) with four rules:

   - Capacity (F15). A store or a park on a side that holds one entry fails
     with CellOccupied before the plan stages it (Cx::check_capacity). The
     job drops the transaction, so the failed operation applies nothing.
   - A fresh element always reaches the head of its side
     (CellDiscipline::reaches_head is true). A send meets the waiter of the
     cell, and a receive meets the record of the cell.
   - A re-issued persistent element keeps the ord of its first round
     (decision 43) and re-enters as a fresh element
     (CellDiscipline::precedes is true, section 43.8 of the design of
     decision 43). An older entry on its side does not hold it back.
   - Rule E (decision 45). A consume-side peek fire removes the
     non-persistent record that it binds, as the host COMM does. The
     default restore of the discipline then stores the same record again,
     with a plain store that has no capacity check.

   The produce plan (offer, with reaches_head = true):
     loop: W := the head waiter, or store (capacity check) and stop
           if W does not match the datum (pattern and guard): store, stop
           stage Fire(datum, W)
           Done if the datum is spent
           re-issue if W stays (A10)
           re-issue at FIRE_ROUND_CAP fires (I-21)
           else offer the datum to the next head waiter (decision 30)
   The consume plan (receive) is symmetric with park. After a peek fire of a
   non-persistent record it stages the re-store of that record.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. cell_capacity_one           F11: the cell plans keep at most one
                                   record and one waiter, and they fail
                                   with CellOccupied exactly when the same
                                   plans without the capacity check would
                                   overflow a side. Otherwise the two plans
                                   are equal.
    2. cell_restore_never_occupied the substrate can apply every staged
                                   operation of a cell plan to the staged
                                   view with the capacity check at every
                                   store, park and re-store. So the re-store
                                   of Rule E meets an empty record slot,
                                   because the peek fire removed the record.
                                   The code stages the re-store without the
                                   check, and the check would never fail.
    3. cell_reentry_as_fresh       decision 43, section 43.8: a re-entered
                                   element with the ord of its first round
                                   runs the plan of a fresh element with
                                   that ord. So it fails with CellOccupied
                                   exactly when a fresh element would.

   Sources
   - Design: the P5 design v3, sections A.7 (the cell row and the table of
     defined effects), A.12 (the cell row) and C.2 (cell restore against
     capacity). The P5 design v2, sections 2.3 (STORE and PARK with the
     capacity check) and 2.8.1 (F11). The design of decision 43, section
     43.8 (the cell keeps its first-round ord, and a re-entered element
     that finds the cell taken fails as a fresh element would).
   - Plan: decisions 11 (head-only), 30 (re-offer of a surviving datum),
     31 (every operation bumps the counter), 37 (the current guard rule),
     39 (F15: a cell holds one record and one waiter), 43 (a re-issued
     persistent operation keeps its first-round ord) and 45 (Rule E).
   - Code: the P5 W2 cell batch over the engine spine S1 (21b2b27fd). The
     line numbers below are the line numbers of that tree.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   cell_capacity            spaces/disciplines/cell.rs:56
                            (CellDiscipline::CAPACITY = Some(1))
   has_room                 spaces/generic.rs:2424-2439 (Cx::check_capacity:
                            no bound does no work, and a side that holds
                            capacity entries fails with CellOccupied)
   cell_reaches_head        spaces/disciplines/cell.rs:71
                            (CellDiscipline::reaches_head is true)
   cell_precedes            spaces/disciplines/cell.rs:76
                            (CellDiscipline::precedes is true)
   precedes_key, head_from, head_index
                            spaces/disciplines/cell.rs:59-70, which calls
                            spaces/disciplines/queue.rs:41-59, 88-123 (the
                            least (ord, bincode bytes), the earlier index
                            on a tie)
   fire_round_cap, after_fire
                            reified/costs.rs:17, 71-82 (FIRE_ROUND_CAP =
                            64, FireRounds::after_fire)
   store_checked, park_checked
                            spaces/generic.rs:3828-3829, 3837-3838, 3847-3848,
                            3859-3860, 3930-3931, 3941-3942, 3951-3952, 3958-3959
                            (check_capacity, then store or park)
   offer, offer_loop        spaces/generic.rs:3817-3904 (offer_head, the
                            default Discipline::offer)
   receive, receive_loop    spaces/generic.rs:3913-4025 (receive_head, the
                            default Discipline::receive)
   fire_view, restores, receive_ops, receive_fired
                            spaces/generic.rs:4110-4115, 1852-1881 and
                            1853-1863 (the copy of a peeked non-persistent
                            record, the fire, and restore_peeked), the
                            default Discipline::restore at
                            spaces/generic.rs:435-448 and Cx::restore at
                            869-880 (a plain store_on with no capacity
                            check). The substrate removes a peeked
                            non-persistent record (decision 45).
   bump_ord, bumped, produce, consume
                            spaces/generic.rs:2543-2710 (run_with: bump
                            the counter, then the plan with the ord of the
                            bump) and spaces/counter.rs (the ord is the
                            value of the counter, and the first operation
                            takes 0)
   operation_ord            spaces/generic.rs:1908-1942 (a re-issued ord
                            lies below the value of the counter, else
                            Inconsistent)
   reentry_at_head, record_head_ord, waiter_head_ord
                            spaces/generic.rs:1083-1110 and 1050-1072
                            (Cx::at_head and Cx::head_ord: an empty side,
                            or the rule precedes, and Inconsistent on an
                            equal ord)
   reenter_offer, reenter_receive, reenter_produce, reenter_consume
                            spaces/generic.rs:3817-3904, 3913-4025 and
                            5451-5497 (the plans with a reissued_ord)
   step_view, replay_view   the substrate applies each staged operation to
                            the staged view: a store and a re-store put a
                            record at the front of its list, a park puts a
                            waiter at the front of its list, and a fire
                            removes each bound entry that is not
                            persistent, by its identity
                            (rspace/space_play.rs, hot_store.rs put_datum,
                            remove_matched_datum_and_join). The model adds
                            the capacity check of Cx::check_capacity at
                            every store, park and re-store.

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - fits w d is the metered match of the payload of d against the pattern
     of w, followed by the guard of w (SpaceMatcher::matches,
     spaces/matching.rs:107-121). The model takes it as a section variable:
     every theorem holds for every matcher.
   - The bincode bytes of an entry give a strict total order on distinct
     entries. The model writes rank for the position of those bytes in that
     order. Two entries with one (ord, rank) are byte-identical, so the
     substrate removes either of them by identity.
   - capacity is an option: Some 1 for the cell, and None for the same
     plans without the capacity check. Theorem 1 compares the two.
   - The staged view of a collection is a record list and a waiter list. A
     fire of a non-persistent partner removes it at its view index.
   - The ord is a nat. The code checks i64 overflow with CounterOverflow
     (spaces/counter.rs), so every ord of the code is a nat.
   - The model gives Occupied for CellOccupied and Inconsistent for the
     Inconsistent errors of operation_ord and at_head.

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Design note
   ─────────────────────────────────────────────────────────────────────────
   - The proofs of theorem 1 compute has_room in place. A separate lemma that
     restated the check (has_room cell_capacity n = (n =? 0)) would come
     first in the file. Then a mutant of the capacity check would fail in
     that lemma, outside the proofs of the headline.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - In the replay of a checked store, simpl before rewrite Hroom unfolds
     has_room cell_capacity, so the rewrite finds no subterm. Use
     cbn [o_ops o_state replay_view step_view], which unfolds only the
     replay, and then rewrite.
   - In receive_fire_replays, destruct (r_pers x) before simpl leaves a
     second occurrence of r_pers x, which step_view exposes later. Use
     destruct (r_pers x) eqn:Hpers, and rewrite ?Hpers after simpl.
   - Replay lemmas with the premise plan = Planned o were hard to drive,
     because the tactics act on the hypothesis. State them over the
     predicate replays st plan on the goal, as the lemmas of theorem 1 do
     with capacity_spec. Then simpl and destruct act on the goal.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
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

(* ── 2. The rules of the cell ──────────────────────────────────────────── *)

(* CellDiscipline::CAPACITY: one entry on each side (F15). *)
Definition cell_capacity : option nat := Some 1.

(* Cx::check_capacity: a side that holds [held] entries has room for one
   more entry. Without a bound a side always has room. *)
Definition has_room (capacity : option nat) (held : nat) : bool :=
  match capacity with
  | None => true
  | Some bound => held <? bound
  end.

(* CellDiscipline::reaches_head: a fresh element always reaches the head of
   its side, whatever the number of queued entries. *)
Definition cell_reaches_head (queued : nat) : bool := true.

(* CellDiscipline::precedes: a re-entered element is at the head, whatever
   the ord of the head of its side. *)
Definition cell_precedes (ord head : nat) : bool := true.

Section Cell.
  Variable V : Type.     (* the payload of a datum *)
  Variable P : Type.     (* the pattern and guard of a continuation *)
  Variable fits : P -> V -> bool.

  Record record : Type := mkRecord { r_ord : nat; r_rank : nat; r_val : V; r_pers : bool }.
  Record waiter : Type :=
    mkWaiter { w_ord : nat; w_rank : nat; w_pat : P; w_pers : bool; w_peek : bool }.

  (* ── 3. Heads ──────────────────────────────────────────────────────── *)

  (* A lesser ord, or an equal ord and lesser bytes. *)
  Definition precedes_key (o1 r1 o2 r2 : nat) : bool :=
    (o1 <? o2) || ((o1 =? o2) && (r1 <? r2)).

  Section Heads.
    Variable E : Type.
    Variable ord : E -> nat.
    Variable rank : E -> nat.

    (* QueueDiscipline::head_metered, which the cell calls: the first entry
       with the least key in list order. A later entry replaces the best
       only when it precedes it. *)
    Fixpoint head_from (best : option (nat * E)) (idx : nat) (l : list E)
        : option (nat * E) :=
      match l with
      | [] => best
      | e :: rest =>
          let best' :=
            match best with
            | None => Some (idx, e)
            | Some (bi, b) =>
                if precedes_key (ord e) (rank e) (ord b) (rank b)
                then Some (idx, e) else Some (bi, b)
            end in
          head_from best' (S idx) rest
      end.

    Definition head_index (l : list E) : option (nat * E) := head_from None 0 l.

    Lemma head_from_in :
      forall l best idx bi b,
        head_from best idx l = Some (bi, b) -> best = Some (bi, b) \/ In b l.
    Proof.
      induction l as [| e rest IH]; intros best idx bi b H; simpl in H.
      - left. exact H.
      - destruct best as [[bj c] |].
        + destruct (precedes_key (ord e) (rank e) (ord c) (rank c)).
          * destruct (IH _ _ _ _ H) as [Hb | Hin].
            -- injection Hb as <- <-. right. left. reflexivity.
            -- right. right. exact Hin.
          * destruct (IH _ _ _ _ H) as [Hb | Hin].
            -- left. exact Hb.
            -- right. right. exact Hin.
        + destruct (IH _ _ _ _ H) as [Hb | Hin].
          * injection Hb as <- <-. right. left. reflexivity.
          * right. right. exact Hin.
    Qed.

    (* The head is an entry of its side. *)
    Lemma head_index_in : forall l bi b, head_index l = Some (bi, b) -> In b l.
    Proof.
      intros l bi b H. destruct (head_from_in l None 0 bi b H) as [H1 | H1].
      - discriminate H1.
      - exact H1.
    Qed.

    (* On a side with one entry at most, the head is the entry at index 0. *)
    Lemma head_index_short :
      forall l,
        length l <= 1 ->
        head_index l = match l with [] => None | e :: _ => Some (0, e) end.
    Proof.
      intros [| e [| e' rest]] H; simpl in H.
      - reflexivity.
      - reflexivity.
      - lia.
    Qed.
  End Heads.

  Arguments head_index {E} ord rank l.
  Arguments head_index_in {E} ord rank l bi b _.
  Arguments head_index_short {E} ord rank l _.

  (* ── 4. States, staged operations and plans ────────────────────────── *)

  (* A collection: its records, its waiting continuations, and its counter
     (None before the first operation). *)
  Record cstate : Type := mkC {
    c_records : list record;
    c_waiters : list waiter;
    c_counter : option nat
  }.

  (* F15: one record and one waiter at most. *)
  Definition cell_wf (st : cstate) : Prop :=
    length (c_records st) <= 1 /\ length (c_waiters st) <= 1.

  (* A staged operation of a plan, after the bump. SRestore is the re-store
     of Rule E. *)
  Inductive staged : Type :=
  | SStore : record -> staged
  | SPark : waiter -> staged
  | SFireProduce : waiter -> record -> staged
  | SFireConsume : waiter -> record -> staged
  | SRestore : record -> staged.

  Record outcome : Type := mkOutcome {
    o_ops : list staged;
    o_state : cstate;
    o_reissue : option reissue
  }.

  (* The result of a plan: CellOccupied, Inconsistent, or the staged
     operations with the staged view after them and the re-issue. *)
  Inductive result : Type :=
  | Occupied : result
  | Inconsistent : result
  | Planned : outcome -> result.

  Fixpoint remove_at {A : Type} (i : nat) (l : list A) : list A :=
    match l, i with
    | [], _ => []
    | _ :: rest, 0 => rest
    | x :: rest, S j => x :: remove_at j rest
    end.

  Definition store_record (st : cstate) (r : record) : cstate :=
    mkC (r :: c_records st) (c_waiters st) (c_counter st).

  Definition park_waiter (st : cstate) (w : waiter) : cstate :=
    mkC (c_records st) (w :: c_waiters st) (c_counter st).

  (* Cx::check_capacity, then Cx::store. *)
  Definition store_checked (capacity : option nat) (st : cstate) (r : record) : result :=
    if has_room capacity (length (c_records st))
    then Planned (mkOutcome [SStore r] (store_record st r) None)
    else Occupied.

  (* Cx::check_capacity, then Cx::park. *)
  Definition park_checked (capacity : option nat) (st : cstate) (k : waiter) : result :=
    if has_room capacity (length (c_waiters st))
    then Planned (mkOutcome [SPark k] (park_waiter st k) None)
    else Occupied.

  (* The operations of a fire before the rest of a loop. A failure of the
     rest fails the whole operation. *)
  Definition prepend (ops : list staged) (res : result) : result :=
    match res with
    | Planned o => Planned (mkOutcome (ops ++ o_ops o) (o_state o) (o_reissue o))
    | other => other
    end.

  (* The staged view after a produce-side fire of the waiter w at index wi:
     the fire removes w unless it is persistent. The datum is never stored
     (decision 30). *)
  Definition offer_fired (st : cstate) (wi : nat) (w : waiter) : cstate :=
    if w_pers w then st else mkC (c_records st) (remove_at wi (c_waiters st)) (c_counter st).

  (* The loop of offer_head. *)
  Fixpoint offer_loop (capacity : option nat) (fuel fired : nat) (st : cstate) (r : record)
      : result :=
    match head_index w_ord w_rank (c_waiters st) with
    | None => store_checked capacity st r
    | Some (wi, w) =>
        if negb (fits (w_pat w) (r_val r)) then store_checked capacity st r
        else
          let st1 := offer_fired st wi w in
          let ops := [SFireProduce w r] in
          match after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w) with
          | FDone => Planned (mkOutcome ops st1 None)
          | FDivergent => Planned (mkOutcome ops st1 (Some ReissueDivergent))
          | FRoundCap => Planned (mkOutcome ops st1 (Some ReissueRoundCap))
          | FContinue =>
              match fuel with
              | 0 => Planned (mkOutcome ops st1 (Some ReissueRoundCap))
              | S fuel' => prepend ops (offer_loop capacity fuel' (S fired) st1 r)
              end
          end
    end.

  (* The staged view after a consume fire of the record h at index hi: the
     substrate removes h unless it is persistent, also for a peek (decision
     45, Rule E). *)
  Definition fire_view (st : cstate) (hi : nat) (h : record) : cstate :=
    if r_pers h then st else mkC (remove_at hi (c_records st)) (c_waiters st) (c_counter st).

  (* Rule E: the code stores a peeked non-persistent record again. *)
  Definition restores (k : waiter) (h : record) : bool := w_peek k && negb (r_pers h).

  (* The staged operations of a consume fire: the fire, then the re-store
     of Rule E (restore_peeked). *)
  Definition receive_ops (k : waiter) (h : record) : list staged :=
    if restores k h then [SFireConsume k h; SRestore h] else [SFireConsume k h].

  (* The staged view after a consume fire and its re-store. The default
     Discipline::restore is a plain store with no capacity check. *)
  Definition receive_fired (k : waiter) (st : cstate) (hi : nat) (h : record) : cstate :=
    if restores k h then store_record (fire_view st hi h) h else fire_view st hi h.

  (* The loop of receive_head. *)
  Fixpoint receive_loop (capacity : option nat) (fuel fired : nat) (st : cstate) (k : waiter)
      : result :=
    match head_index r_ord r_rank (c_records st) with
    | None => park_checked capacity st k
    | Some (hi, h) =>
        if negb (fits (w_pat k) (r_val h)) then park_checked capacity st k
        else
          let st1 := receive_fired k st hi h in
          let ops := receive_ops k h in
          match after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k) with
          | FDone => Planned (mkOutcome ops st1 None)
          | FDivergent => Planned (mkOutcome ops st1 (Some ReissueDivergent))
          | FRoundCap => Planned (mkOutcome ops st1 (Some ReissueRoundCap))
          | FContinue =>
              match fuel with
              | 0 => Planned (mkOutcome ops st1 (Some ReissueRoundCap))
              | S fuel' => prepend ops (receive_loop capacity fuel' (S fired) st1 k)
              end
          end
    end.

  (* offer_head for a fresh element: Cx::at_head with no reissued_ord asks
     reaches_head with the number of records. *)
  Definition offer (capacity : option nat) (st : cstate) (r : record) : result :=
    if cell_reaches_head (length (c_records st))
    then offer_loop capacity fire_round_cap 0 st r
    else store_checked capacity st r.

  (* receive_head for a fresh element. *)
  Definition receive (capacity : option nat) (st : cstate) (k : waiter) : result :=
    if cell_reaches_head (length (c_waiters st))
    then receive_loop capacity fire_round_cap 0 st k
    else park_checked capacity st k.

  (* run_with: the bump, then the plan with the ord of the bump. rank is the
     byte rank of the new entry. *)
  Definition bump_ord (counter : option nat) : nat :=
    match counter with None => 0 | Some v => v end.

  Definition bumped (st : cstate) : cstate :=
    mkC (c_records st) (c_waiters st) (Some (S (bump_ord (c_counter st)))).

  Definition produce (capacity : option nat) (st : cstate) (rank : nat) (v : V) (pers : bool)
      : result :=
    offer capacity (bumped st) (mkRecord (bump_ord (c_counter st)) rank v pers).

  Definition consume (capacity : option nat) (st : cstate) (rank : nat) (pat : P)
      (pers peek : bool) : result :=
    receive capacity (bumped st) (mkWaiter (bump_ord (c_counter st)) rank pat pers peek).

  (* ── 5. Re-entry (decision 43) ─────────────────────────────────────── *)

  (* operation_ord: a re-issued ord lies below the value of the counter
     before the bump, else Inconsistent. *)
  Definition operation_ord (reissued : nat) (counter : option nat) : option nat :=
    if reissued <? bump_ord counter then Some reissued else None.

  (* Cx::at_head for a re-entered element with the first-round ord o: an
     empty side, or the rule precedes of the cell. A stored entry with the
     ord o is Inconsistent. *)
  Definition reentry_at_head (o : nat) (head : option nat) : option bool :=
    match head with
    | None => Some true
    | Some h => if o =? h then None else Some (cell_precedes o h)
    end.

  Definition record_head_ord (st : cstate) : option nat :=
    match head_index r_ord r_rank (c_records st) with
    | None => None
    | Some (_, h) => Some (r_ord h)
    end.

  Definition waiter_head_ord (st : cstate) : option nat :=
    match head_index w_ord w_rank (c_waiters st) with
    | None => None
    | Some (_, w) => Some (w_ord w)
    end.

  (* offer_head for a re-entered datum r, which carries its first-round
     ord. *)
  Definition reenter_offer (capacity : option nat) (st : cstate) (r : record) : result :=
    match reentry_at_head (r_ord r) (record_head_ord st) with
    | None => Inconsistent
    | Some true => offer_loop capacity fire_round_cap 0 st r
    | Some false => store_checked capacity st r
    end.

  (* receive_head for a re-entered continuation k. *)
  Definition reenter_receive (capacity : option nat) (st : cstate) (k : waiter) : result :=
    match reentry_at_head (w_ord k) (waiter_head_ord st) with
    | None => Inconsistent
    | Some true => receive_loop capacity fire_round_cap 0 st k
    | Some false => park_checked capacity st k
    end.

  (* run_with with a reissued_ord: the bump, the check of operation_ord, and
     the plan with the first-round ord. *)
  Definition reenter_produce (capacity : option nat) (st : cstate) (r : record) : result :=
    match operation_ord (r_ord r) (c_counter st) with
    | None => Inconsistent
    | Some _ => reenter_offer capacity (bumped st) r
    end.

  Definition reenter_consume (capacity : option nat) (st : cstate) (k : waiter) : result :=
    match operation_ord (w_ord k) (c_counter st) with
    | None => Inconsistent
    | Some _ => reenter_receive capacity (bumped st) k
    end.

  (* ── 6. The capacity of the plans (F11) ────────────────────────────── *)

  (* A side holds more than one entry. *)
  Definition overflows (o : outcome) : Prop :=
    1 < length (c_records (o_state o)) \/ 1 < length (c_waiters (o_state o)).

  (* The checked plan fails with CellOccupied exactly when the unbounded plan
     overflows a side. Otherwise the two plans are equal, and the staged
     view after them is well formed. *)
  Definition capacity_spec (checked unbounded : result) : Prop :=
    (checked = Occupied <-> exists o, unbounded = Planned o /\ overflows o) /\
    (forall o, checked = Planned o -> unbounded = Planned o /\ cell_wf (o_state o)) /\
    checked <> Inconsistent.

  Lemma remove_at_length_le : forall (A : Type) (i : nat) (l : list A), length (remove_at i l) <= length l.
  Proof.
    intros A i l. revert i. induction l as [| x rest IH]; intros [| j]; simpl; try lia.
    specialize (IH j). lia.
  Qed.

  Lemma planned_spec : forall o, cell_wf (o_state o) -> capacity_spec (Planned o) (Planned o).
  Proof.
    intros o [Hr Hw]. split; [| split].
    - split.
      + intros H. discriminate H.
      + intros [o' [Ho' Hov]]. injection Ho' as <-. unfold overflows in Hov. lia.
    - intros o' Ho'. injection Ho' as <-. split; [reflexivity | split; assumption].
    - discriminate.
  Qed.

  Lemma prepend_spec :
    forall ops checked unbounded,
      capacity_spec checked unbounded -> capacity_spec (prepend ops checked) (prepend ops unbounded).
  Proof.
    intros ops checked unbounded [[Hocc1 Hocc2] [Hpl Hinc]].
    destruct checked as [| | o].
    - destruct (Hocc1 eq_refl) as [o [Hu Hov]]. subst unbounded. simpl. split; [| split].
      + split; [intros _ | intros _; reflexivity].
        exists (mkOutcome (ops ++ o_ops o) (o_state o) (o_reissue o)). split; [reflexivity |].
        exact Hov.
      + intros o' H. discriminate H.
      + discriminate.
    - exfalso. apply Hinc. reflexivity.
    - destruct (Hpl o eq_refl) as [Hu Hwf]. subst unbounded. simpl.
      apply planned_spec. exact Hwf.
  Qed.

  (* The terminal store of offer_head. The proof computes the capacity check
     in place (see the failed strategies). *)
  Lemma store_checked_spec :
    forall st r, cell_wf st -> capacity_spec (store_checked cell_capacity st r) (store_checked None st r).
  Proof.
    intros st r [Hr Hw]. unfold store_checked, cell_capacity, has_room.
    destruct (c_records st) as [| x [| y rest]] eqn:Erec; simpl in Hr; try lia.
    - simpl. apply planned_spec. unfold cell_wf, store_record. simpl. rewrite Erec. simpl. lia.
    - simpl. split; [| split].
      + split; [intros _ | intros _; reflexivity].
        exists (mkOutcome [SStore r] (store_record st r) None). split; [reflexivity |].
        left. unfold store_record. simpl. rewrite Erec. simpl. lia.
      + intros o H. discriminate H.
      + discriminate.
  Qed.

  (* The terminal park of receive_head. *)
  Lemma park_checked_spec :
    forall st k, cell_wf st -> capacity_spec (park_checked cell_capacity st k) (park_checked None st k).
  Proof.
    intros st k [Hr Hw]. unfold park_checked, cell_capacity, has_room.
    destruct (c_waiters st) as [| x [| y rest]] eqn:Ewait; simpl in Hw; try lia.
    - simpl. apply planned_spec. unfold cell_wf, park_waiter. simpl. rewrite Ewait. simpl. lia.
    - simpl. split; [| split].
      + split; [intros _ | intros _; reflexivity].
        exists (mkOutcome [SPark k] (park_waiter st k) None). split; [reflexivity |].
        right. unfold park_waiter. simpl. rewrite Ewait. simpl. lia.
      + intros o H. discriminate H.
      + discriminate.
  Qed.

  Lemma offer_fired_wf : forall st wi w, cell_wf st -> cell_wf (offer_fired st wi w).
  Proof.
    intros st wi w [Hr Hw]. unfold offer_fired. destruct (w_pers w).
    - split; assumption.
    - split; simpl; [exact Hr |]. pose proof (remove_at_length_le _ wi (c_waiters st)). lia.
  Qed.

  (* A fire on a well-formed cell removes the head record at index 0 of a
     one-record side, so the re-store meets an empty record slot. *)
  Lemma receive_fired_wf :
    forall st hi h k,
      cell_wf st -> head_index r_ord r_rank (c_records st) = Some (hi, h) ->
      cell_wf (receive_fired k st hi h).
  Proof.
    intros st hi h k [Hr Hw] Hh.
    rewrite (head_index_short r_ord r_rank (c_records st) Hr) in Hh.
    destruct (c_records st) as [| x [| y rest]] eqn:Erec; simpl in Hr; try lia.
    - discriminate Hh.
    - injection Hh as <- <-. unfold receive_fired, restores, fire_view.
      destruct (w_peek k); destruct (r_pers x); simpl;
        unfold cell_wf, store_record; simpl; try rewrite Erec; simpl; lia.
  Qed.

  Lemma offer_loop_capacity :
    forall fuel fired st r,
      cell_wf st ->
      capacity_spec (offer_loop cell_capacity fuel fired st r) (offer_loop None fuel fired st r).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hwf; simpl;
      destruct (head_index w_ord w_rank (c_waiters st)) as [[wi w] |];
      try (apply store_checked_spec; exact Hwf);
      destruct (negb (fits (w_pat w) (r_val r)));
      try (apply store_checked_spec; exact Hwf);
      pose proof (offer_fired_wf st wi w Hwf) as Hwf1;
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      try (apply planned_spec; exact Hwf1).
    apply prepend_spec. apply IH. exact Hwf1.
  Qed.

  Lemma receive_loop_capacity :
    forall fuel fired st k,
      cell_wf st ->
      capacity_spec (receive_loop cell_capacity fuel fired st k) (receive_loop None fuel fired st k).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hwf; simpl;
      destruct (head_index r_ord r_rank (c_records st)) as [[hi h] |] eqn:Hh;
      try (apply park_checked_spec; exact Hwf);
      destruct (negb (fits (w_pat k) (r_val h)));
      try (apply park_checked_spec; exact Hwf);
      pose proof (receive_fired_wf st hi h k Hwf Hh) as Hwf1;
      destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
      try (apply planned_spec; exact Hwf1).
    apply prepend_spec. apply IH. exact Hwf1.
  Qed.

  Lemma bumped_wf : forall st, cell_wf st -> cell_wf (bumped st).
  Proof. intros st [Hr Hw]. split; assumption. Qed.

  (* Headline 1 (F11): the cell plans keep at most one record and one
     waiter. A plan fails with CellOccupied exactly when the same plan
     without the capacity check overflows a side, and otherwise the two
     plans are equal. This holds for every element of offer and receive,
     so for a fresh operation and, by headline 3, for a re-entered one. *)
  Theorem cell_capacity_one :
    forall st,
      cell_wf st ->
      (forall r, capacity_spec (offer cell_capacity st r) (offer None st r)) /\
      (forall k, capacity_spec (receive cell_capacity st k) (receive None st k)) /\
      (forall rank v pers,
          capacity_spec (produce cell_capacity st rank v pers) (produce None st rank v pers)) /\
      (forall rank pat pers peek,
          capacity_spec (consume cell_capacity st rank pat pers peek)
                        (consume None st rank pat pers peek)).
  Proof.
    assert (Hoffer : forall st r, cell_wf st -> capacity_spec (offer cell_capacity st r) (offer None st r)).
    { intros st r Hwf. unfold offer. destruct (cell_reaches_head (length (c_records st))).
      - apply offer_loop_capacity. exact Hwf.
      - apply store_checked_spec. exact Hwf. }
    assert (Hreceive : forall st k, cell_wf st -> capacity_spec (receive cell_capacity st k) (receive None st k)).
    { intros st k Hwf. unfold receive. destruct (cell_reaches_head (length (c_waiters st))).
      - apply receive_loop_capacity. exact Hwf.
      - apply park_checked_spec. exact Hwf. }
    intros st Hwf. split; [| split; [| split]].
    - intros r. apply Hoffer. exact Hwf.
    - intros k. apply Hreceive. exact Hwf.
    - intros rank v pers. unfold produce. apply Hoffer. apply bumped_wf. exact Hwf.
    - intros rank pat pers peek. unfold consume. apply Hreceive. apply bumped_wf. exact Hwf.
  Qed.

  (* ── 7. The re-store of Rule E meets an empty slot ─────────────────── *)

  (* The substrate removes a fired entry by its identity: the first entry
     with its (ord, rank). *)
  Fixpoint remove_record (h : record) (l : list record) : list record :=
    match l with
    | [] => []
    | x :: rest =>
        if (r_ord x =? r_ord h) && (r_rank x =? r_rank h) then rest
        else x :: remove_record h rest
    end.

  Fixpoint remove_waiter (w : waiter) (l : list waiter) : list waiter :=
    match l with
    | [] => []
    | x :: rest =>
        if (w_ord x =? w_ord w) && (w_rank x =? w_rank w) then rest
        else x :: remove_waiter w rest
    end.

  (* One staged operation applied to the staged view. Cx::check_capacity
     guards every store and park. The model also guards the re-store of
     Rule E, which the code stages without a check. A fire removes each
     bound entry that is not persistent, also a peeked record (decision 45,
     Rule E). None: a check fails. *)
  Definition step_view (v : cstate) (s : staged) : option cstate :=
    match s with
    | SStore r =>
        if has_room cell_capacity (length (c_records v)) then Some (store_record v r) else None
    | SPark w =>
        if has_room cell_capacity (length (c_waiters v)) then Some (park_waiter v w) else None
    | SFireProduce w _ =>
        Some (if w_pers w then v else mkC (c_records v) (remove_waiter w (c_waiters v)) (c_counter v))
    | SFireConsume _ h =>
        Some (if r_pers h then v else mkC (remove_record h (c_records v)) (c_waiters v) (c_counter v))
    | SRestore h =>
        if has_room cell_capacity (length (c_records v)) then Some (store_record v h) else None
    end.

  Fixpoint replay_view (v : cstate) (ops : list staged) : option cstate :=
    match ops with
    | [] => Some v
    | s :: rest =>
        match step_view v s with
        | Some v' => replay_view v' rest
        | None => None
        end
    end.

  Lemma replay_view_app :
    forall ops1 ops2 v,
      replay_view v (ops1 ++ ops2) =
      match replay_view v ops1 with Some v' => replay_view v' ops2 | None => None end.
  Proof.
    induction ops1 as [| s rest IH]; intros ops2 v; simpl.
    - reflexivity.
    - destruct (step_view v s); [apply IH | reflexivity].
  Qed.

  (* The plan result [res] replays from the staged view [st]: every staged
     operation applies with its checks, and the replay ends in the staged
     view of the plan. A failed plan stages nothing. *)
  Definition replays (st : cstate) (res : result) : Prop :=
    match res with
    | Planned o => replay_view st (o_ops o) = Some (o_state o)
    | Occupied | Inconsistent => True
    end.

  Lemma prepend_replays :
    forall ops res st st1,
      replay_view st ops = Some st1 -> replays st1 res -> replays st (prepend ops res).
  Proof.
    intros ops [| | o] st st1 Hops Hres; simpl; try exact I.
    rewrite replay_view_app, Hops. exact Hres.
  Qed.

  Lemma store_checked_replays : forall st r, replays st (store_checked cell_capacity st r).
  Proof.
    intros st r. unfold store_checked.
    destruct (has_room cell_capacity (length (c_records st))) eqn:Hroom; [| exact I].
    unfold replays. cbn [o_ops o_state replay_view step_view]. rewrite Hroom. reflexivity.
  Qed.

  Lemma park_checked_replays : forall st k, replays st (park_checked cell_capacity st k).
  Proof.
    intros st k. unfold park_checked.
    destruct (has_room cell_capacity (length (c_waiters st))) eqn:Hroom; [| exact I].
    unfold replays. cbn [o_ops o_state replay_view step_view]. rewrite Hroom. reflexivity.
  Qed.

  (* A produce-side fire on a well-formed cell removes the one waiter by its
     identity, as the plan removes it at index 0. *)
  Lemma offer_fire_replays :
    forall st wi w r,
      cell_wf st -> head_index w_ord w_rank (c_waiters st) = Some (wi, w) ->
      replay_view st [SFireProduce w r] = Some (offer_fired st wi w).
  Proof.
    intros st wi w r [Hr Hw] Hh.
    rewrite (head_index_short w_ord w_rank (c_waiters st) Hw) in Hh.
    destruct (c_waiters st) as [| x [| y rest]] eqn:Ewait; simpl in Hw; try lia.
    - discriminate Hh.
    - injection Hh as <- <-. unfold offer_fired. cbn [replay_view step_view].
      destruct (w_pers x); [reflexivity |].
      rewrite Ewait. simpl. rewrite !Nat.eqb_refl. reflexivity.
  Qed.

  (* A consume fire on a well-formed cell removes the one record by its
     identity. After a peek of a non-persistent record the record slot is
     empty, so the checked re-store succeeds and ends in the staged view of
     the plan. *)
  Lemma receive_fire_replays :
    forall st hi h k,
      cell_wf st -> head_index r_ord r_rank (c_records st) = Some (hi, h) ->
      replay_view st (receive_ops k h) = Some (receive_fired k st hi h).
  Proof.
    intros st hi h k [Hr Hw] Hh.
    rewrite (head_index_short r_ord r_rank (c_records st) Hr) in Hh.
    destruct (c_records st) as [| x [| y rest]] eqn:Erec; simpl in Hr; try lia.
    - discriminate Hh.
    - injection Hh as <- <-. unfold receive_ops, receive_fired, restores, fire_view.
      destruct (w_peek k); destruct (r_pers x) eqn:Hpers; simpl; rewrite ?Hpers, ?Erec; simpl;
        rewrite ?Nat.eqb_refl; reflexivity.
  Qed.

  Lemma offer_loop_replays :
    forall fuel fired st r, cell_wf st -> replays st (offer_loop cell_capacity fuel fired st r).
  Proof.
    induction fuel as [| fuel IH]; intros fired st r Hwf; simpl;
      destruct (head_index w_ord w_rank (c_waiters st)) as [[wi w] |] eqn:Hh;
      try apply store_checked_replays;
      destruct (negb (fits (w_pat w) (r_val r)));
      try apply store_checked_replays;
      pose proof (offer_fired_wf st wi w Hwf) as Hwf1;
      pose proof (offer_fire_replays st wi w r Hwf Hh) as Hfire;
      destruct (after_fire (S fired) (negb (r_pers r) && negb (w_peek w)) (w_pers w));
      try exact Hfire.
    apply (prepend_replays _ _ _ _ Hfire). apply IH. exact Hwf1.
  Qed.

  Lemma receive_loop_replays :
    forall fuel fired st k, cell_wf st -> replays st (receive_loop cell_capacity fuel fired st k).
  Proof.
    induction fuel as [| fuel IH]; intros fired st k Hwf; simpl;
      destruct (head_index r_ord r_rank (c_records st)) as [[hi h] |] eqn:Hh;
      try apply park_checked_replays;
      destruct (negb (fits (w_pat k) (r_val h)));
      try apply park_checked_replays;
      pose proof (receive_fired_wf st hi h k Hwf Hh) as Hwf1;
      pose proof (receive_fire_replays st hi h k Hwf Hh) as Hfire;
      destruct (after_fire (S fired) (negb (w_pers k)) (r_pers h || w_peek k));
      try exact Hfire.
    apply (prepend_replays _ _ _ _ Hfire). apply IH. exact Hwf1.
  Qed.

  (* Headline 2: every staged operation of a cell plan applies to the
     staged view at its point with the capacity check of Cx::check_capacity.
     In particular the re-store of Rule E meets an empty record slot,
     because the peek fire removed the record and the cell held one record.
     So the plain re-store of the code (Discipline::restore) never needs
     the check, and the check would never report CellOccupied. *)
  Theorem cell_restore_never_occupied :
    forall st,
      cell_wf st ->
      (forall r o, offer cell_capacity st r = Planned o ->
         replay_view st (o_ops o) = Some (o_state o)) /\
      (forall k o, receive cell_capacity st k = Planned o ->
         replay_view st (o_ops o) = Some (o_state o)).
  Proof.
    intros st Hwf. split.
    - intros r o Hplan.
      assert (Hreplays : replays st (offer cell_capacity st r)).
      { unfold offer. destruct (cell_reaches_head (length (c_records st))).
        - apply offer_loop_replays. exact Hwf.
        - apply store_checked_replays. }
      rewrite Hplan in Hreplays. exact Hreplays.
    - intros k o Hplan.
      assert (Hreplays : replays st (receive cell_capacity st k)).
      { unfold receive. destruct (cell_reaches_head (length (c_waiters st))).
        - apply receive_loop_replays. exact Hwf.
        - apply park_checked_replays. }
      rewrite Hplan in Hreplays. exact Hreplays.
  Qed.

  (* ── 8. Re-entry as a fresh element (decision 43, section 43.8) ────── *)

  (* Headline 3: a re-entered element with a valid first-round ord runs the
     plan of a fresh element with that ord on the bumped collection. So it
     keeps its record and its identity, it fires as a fresh element fires,
     and it fails with CellOccupied exactly when a fresh element would. The
     premises are the checks of operation_ord and at_head: the ord lies
     below the counter, and no stored entry of its side has it. *)
  Theorem cell_reentry_as_fresh :
    forall capacity st,
      (forall r,
          r_ord r < bump_ord (c_counter st) ->
          (forall e, In e (c_records st) -> r_ord e <> r_ord r) ->
          reenter_produce capacity st r = offer capacity (bumped st) r) /\
      (forall k,
          w_ord k < bump_ord (c_counter st) ->
          (forall e, In e (c_waiters st) -> w_ord e <> w_ord k) ->
          reenter_consume capacity st k = receive capacity (bumped st) k).
  Proof.
    intros capacity st. split.
    - intros r Hlt Hdistinct. unfold reenter_produce, operation_ord.
      rewrite (proj2 (Nat.ltb_lt _ _) Hlt).
      unfold reenter_offer, offer, record_head_ord, reentry_at_head, cell_precedes,
        cell_reaches_head.
      simpl c_records.
      destruct (head_index r_ord r_rank (c_records st)) as [[hi h] |] eqn:Hh.
      + assert (Hne : r_ord r =? r_ord h = false).
        { apply Nat.eqb_neq. intros Heq. apply (Hdistinct h).
          - exact (head_index_in r_ord r_rank _ _ _ Hh).
          - symmetry. exact Heq. }
        rewrite Hne. reflexivity.
      + reflexivity.
    - intros k Hlt Hdistinct. unfold reenter_consume, operation_ord.
      rewrite (proj2 (Nat.ltb_lt _ _) Hlt).
      unfold reenter_receive, receive, waiter_head_ord, reentry_at_head, cell_precedes,
        cell_reaches_head.
      simpl c_waiters.
      destruct (head_index w_ord w_rank (c_waiters st)) as [[wi w] |] eqn:Hh.
      + assert (Hne : w_ord k =? w_ord w = false).
        { apply Nat.eqb_neq. intros Heq. apply (Hdistinct w).
          - exact (head_index_in w_ord w_rank _ _ _ Hh).
          - symmetry. exact Heq. }
        rewrite Hne. reflexivity.
      + reflexivity.
  Qed.
End Cell.

Print Assumptions cell_capacity_one.
Print Assumptions cell_restore_never_occupied.
Print Assumptions cell_reentry_as_fresh.
