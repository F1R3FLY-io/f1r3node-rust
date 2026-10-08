(* ═══════════════════════════════════════════════════════════════════════════
   GenericRSpace.v — the engine of the native spaces with the scan driver and
   the cell (P5a)

   The engine spine S1 of P5 runs every family of native spaces. A plan of a
   discipline reads the staged view of its collection and stages host
   operations through Cx: stores, parks, produce-side fires, consume-side
   group fires over [D; n], and retirements. This module models the plans of
   the scan driver of bag and set (OFFER-SCAN, RECEIVE-SCAN, RESTORE-SCAN and
   RETIRE-CLASS of the P5 design v3, A.7) and the plans of the cell, which
   keeps the head driver of S1 with a capacity of one entry on each side
   (F15). It proves the safety rules of the engine for bag, set and cell:
   - every fire binds a stored partner that matches spatially and that the
     guard admitted (F20, F21);
   - a waiter without a guard never has a pending spatial match with a
     stored record (F22, the FIPS rule of no pending match, restated for the
     current guard rule of decision 37);
   - after a consume-side peek fire, the re-store of the peeked record is the
     second send of the host (Rule E of decision 45): the same record, not
     persistent, with its own authority, through the produce plan;
   - a re-issued operation of a collection without a counter keeps the
     record of its first round, so it keeps its identity (decision 43).

   The produce plan of the scan driver.
     loop: W ← the first waiter, in the canonical order, that matches the
               datum spatially and whose guard admits it
           if none: store the datum, stop
           fire(datum, W); a fire that W does not peek retires the class
               of the datum (set only)
           FireRounds: Done stops, Reissue re-issues the operation, and
               Continue offers the datum to the next waiter (decision 30)
   The consume plan of the scan driver.
     loop: r ← the first record, in the canonical order, that matches
               spatially; if none, or if the guard refuses r: park, stop
           C ← [r] for a peek or a bag, else r and the other members of the
               class of r (F3)
           fire(K, C) over [D; |C|], dispatching the result of r only
           after a peek fire of a non-persistent r: RESTORE-SCAN(r), the
               produce plan of the record r, not persistent, with its own
               authority
           FireRounds as for the produce plan
   The cell plans are the head driver of S1 with the capacity check (F15),
   and a peek of the cell stores the record again with a plain store.

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md A.3.3 and A.3.4 (the
     frozen interfaces and the changed algorithms of S1), A.7 (the scan
     driver pseudocode and the table of defined effects), A.12 and A.13
     (the P5a row); p5-design-v2.md 2.3 (the algorithms) and 2.8.1 (F20,
     F21, F22). The design report of decision 45, sections C.4, C.11.
   - Plan: decisions 11, 30, 37, 41, 42, 43 and 45 (Rule E: a peek removes
     the record, and the same transaction stores it again with its own
     authority and no counter bump).
   - Code: the engine spine S1 at commit 21b2b27fd. The scan driver is
     written by batch BAG in spaces/disciplines/scan.rs, and the cell by
     batch CELL in spaces/disciplines/cell.rs, in the same wave. This module
     models the design, which is the reference.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (paths under rholang/src/rust/interpreter/ unless stated)
   ─────────────────────────────────────────────────────────────────────────
   rec, wtr, state          the staged view of one collection: the records
                            and the waiting continuations of D(sid,k)
                            (Cx::data, Cx::waiters, spaces/generic.rs:
                            740-749)
   elem                     the element of a produce: the record, its
                            persistence and the introduction that its
                            staged operations register (ProduceOp,
                            spaces/agent.rs; Introduction, spaces/record.rs:
                            2115-2159)
   staged, apply            the staging primitives of Cx and their effect
                            on the view (spaces/generic.rs:978-1249):
                            store_on (SStore), park_on (SPark),
                            fire_offer_on (SFireOffer), fire_receive_group
                            (SFireRecv), retire (SRetire). A fire removes
                            each bound non-persistent stored record and a
                            non-persistent waiter, and never stores the
                            incoming datum, as the host does (Rule E,
                            rspace/space_play.rs, replay_rspace/
                            space_replay.rs); put_datum inserts a stored
                            record at the front (rspace/hot_store.rs)
   data_order, waiter_order Cx::canonical_data_order and
                            Cx::canonical_waiter_order
                            (spaces/generic.rs:763-778; DR-75 of decision
                            41, rspace++/src/rspace/space_store_handle.rs:
                            332-364). The model takes any permutation
   after_fire, cap          FireRounds::after_fire and FIRE_ROUND_CAP
                            (reified/costs.rs:41-44, 302-334)
   first_admitting,         OFFER-SCAN (A.7, decision 37); Match.v proves
     offer_loop             that its refused-set scan chooses the first
                            admitting waiter (guard_once_greedy_like_head)
   retire_class             RETIRE-CLASS (A.7, F3, F10): ScanDiscipline::
                            class_key and Cx::retire
   first_spatial,           RECEIVE-SCAN (A.7, decision 37)
     receive_group,
     receive_loop
   restore_elem,            RESTORE-SCAN (A.7, decision 45): the record r,
     restore_scan           persist = false, Introduction::for_record
                            (spaces/record.rs:2145-2158), through the produce
                            plan
   host_resend, second_send the host's second send: produce_peeks
                            (reduce.rs:981-1016) produces the removed datum
                            of each result whose persist flag is false, not
                            persistent, and registers the datum's own cost
                            authority (design report of decision 45, A.1,
                            A.2, C.4)
   cell_offer_loop,         offer_head and receive_head of S1
     cell_receive_loop      (spaces/generic.rs:1505-1578, 1580-1671) with
                            Cx::check_capacity (spaces/generic.rs:932-954)
                            and the plain restore of Discipline::restore
                            (spaces/generic.rs:408-421, 1048-1068). The cell
                            reaches the head on every operation
                            (reaches_head = true, precedes = true)
   operation_ord            operation_ord (spaces/generic.rs:1696-1730):
                            with no counter every order is 0
   scan_round, scan_reissue the record of a round of a scan produce
                            (record::wrap_datum_metered with the order of
                            the round) and the re-issue of A.7
                            (reissued_ord = None)
   restore_reissue          RESTORE-SCAN at the cap (A.7): the re-issue of
                            ProduceOp(unwrap(r))

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - Record bytes, payloads, patterns, guards, bindings, authorities and
     class keys are abstract types. The matcher (spatial, admits), the
     payload decode, the authority of a record, the produce identity and
     the class key are section variables, so every theorem holds for every
     matcher, every guard and every class key. A class key of None for every
     payload gives the bag; the F5 key gives the set.
   - A receive without a where guard has the guard no_guard, which admits
     every binding (SpaceMatcher::admits of a continuation whose guard is
     None).
   - The canonical orders are any permutations of their lists. The model
     needs no other property of DR-75.
   - The produce identity of a record is ident applied to its bytes and its
     persistence (Produce::create hashes the channel, the bytes and the
     flag). The channel is the one data channel of the collection.
   - The fuel of a loop is the number of entries of the side that the loop
     consumes, plus one. Each round that continues removes one entry of that
     side, so the fuel never runs out. The fuel-0 branches are unreachable.
   - The cell holds at most one record and one waiter (cell_inv), so its
     head is the only entry. The model reads the first entry of the list as
     the head, which is the head of the code under cell_inv.
   - A plan of the cell that fails with CellOccupied stages nothing, and the
     job drops the transaction (decision 19, D6): the model returns None.
   - The legacy charges of Option P and the metering of decision 42 are not
     modeled here (LegacyCharges.v, AgentReadCoverage.v).

   ─────────────────────────────────────────────────────────────────────────
   Results (5 headlines, axiom-free)
   ─────────────────────────────────────────────────────────────────────────
   F20 consume_fires_only_matching       every consume plan of the scan
                                         driver and of the cell stages
                                         fires whose dispatched record
                                         matches spatially and passed the
                                         guard, with stored candidates
   F21 produce_fires_only_matching       every produce plan stages fires
                                         whose waiter is stored, matches the
                                         datum spatially and admits it
   F22 unordered_no_pending_match        bag, set and cell plans keep
                                         no_pending (cell: cell_inv), so a
                                         waiter without a guard never meets
                                         a stored spatial match
   peek_restore_equals_host_reproduce    a peek of a non-persistent record
                                         stages the fire and then the
                                         second send of the host, the same
                                         record with its own identity and
                                         authority, through the produce
                                         plan; when the second send stores,
                                         the records are as before; the
                                         plain restore of the cell is the
                                         second send of the cell
   scan_reissue_identity_stable          with no counter every order is 0;
                                         every round of a re-issued scan
                                         produce stages the record of its
                                         first round; a plan re-issues its
                                         own element or the re-send of a
                                         peeked record; the cap re-issue of
                                         a re-send stages the record itself

   Rust correspondence of the extracted tests: rholang/tests/
   reified_engine_props.rs (prop_engine_consume_fires_only_matching,
   prop_engine_produce_fires_only_matching,
   prop_engine_no_guard_free_pending_match,
   prop_engine_peek_restore_equals_host_reproduce,
   prop_engine_scan_reissue_keeps_identity).

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A destructuring let (let (ops, re) := loop ... in ...) in a loop body
     blocks rewriting with the equation of the inner call. The loops use
     fst and snd of the inner call instead.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Arith Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section Engine.
  (* ── 1. The data of a collection ─────────────────────────────────────── *)

  Variable B : Type.     (* the bytes of a record: envelope, payload, random state, authority *)
  Variable B_eq_dec : forall x y : B, {x = y} + {x <> y}.
  Variable V : Type.     (* a payload *)
  Variable P : Type.     (* the pattern of a bind *)
  Variable G : Type.     (* a where guard *)
  Variable M : Type.     (* the bound values of a match *)
  Variable A : Type.     (* a cost authority *)
  Variable K : Type.     (* a class key of the set (F5) *)
  Variable K_eq_dec : forall x y : K, {x = y} + {x <> y}.

  Variable payload : B -> V.          (* record::unwrap_datum: the payload *)
  Variable own_auth : B -> A.         (* the cost authority of the record (Introduction::for_record) *)
  Variable ident : B -> bool -> nat.  (* Produce::create on the data channel *)

  Variable spatial : P -> V -> option M.  (* SpaceMatcher::bind *)
  Variable admits : G -> M -> bool.       (* SpaceMatcher::admits *)
  Variable no_guard : G.                  (* the guard of a receive without a where clause *)
  Hypothesis no_guard_admits : forall m, admits no_guard m = true.

  Variable class_key : V -> option K.     (* ScanDiscipline::class_key *)

  Variable cap : nat.                     (* FIRE_ROUND_CAP *)

  (* A stored record: its bytes and its persistence. *)
  Record rec : Type := mkRec { r_bytes : B; r_pers : bool }.

  (* A waiting continuation: its identity, the pattern of its one bind, its
     guard, its flags, and the introduction authority of its operation. *)
  Record wtr : Type := mkWtr {
    w_key : nat; w_pat : P; w_guard : G; w_pers : bool; w_peek : bool; w_auth : A
  }.

  (* The element of a produce: the record bytes, the persistence, and the
     authority that its staged operations register. *)
  Record elem : Type := mkElem { e_bytes : B; e_pers : bool; e_auth : A }.

  Definition rec_ident (r : rec) : nat := ident (r_bytes r) (r_pers r).
  Definition elem_ident (e : elem) : nat := ident (e_bytes e) (e_pers e).

  (* The staged view of the collection. *)
  Record state : Type := mkState { recs : list rec; wtrs : list wtr }.

  Variable data_order : list rec -> list rec.     (* Cx::canonical_data_order *)
  Hypothesis data_order_perm : forall l, Permutation (data_order l) l.
  Variable waiter_order : list wtr -> list wtr.   (* Cx::canonical_waiter_order *)
  Hypothesis waiter_order_perm : forall l, Permutation (waiter_order l) l.

  (* ── 2. Staged operations and their effects ──────────────────────────── *)

  Inductive staged : Type :=
  | SStore (e : elem)                          (* store_on with the introduction e_auth *)
  | SPark (k : wtr)                            (* park_on *)
  | SFireOffer (e : elem) (w : wtr) (m : M)    (* fire_offer_on: e fires w, binding m *)
  | SFireRecv (k : wtr) (c : list rec) (m : M) (* fire_receive_group: k fires over c, dispatching m *)
  | SRetire (r : rec).                         (* retire: K_retire of the member r *)

  Definition rec_eq_dec : forall x y : rec, {x = y} + {x <> y}.
  Proof.
    intros [bx px] [by' py].
    destruct (B_eq_dec bx by') as [Eb | Nb]; [| right; intros H; injection H as Hb _; contradiction].
    destruct (Bool.bool_dec px py) as [Ep | Np]; [left; subst; reflexivity |].
    right; intros H; injection H as _ Hp; contradiction.
  Defined.

  Definition rec_eqb (x y : rec) : bool := if rec_eq_dec x y then true else false.

  (* Removes the first copy of r. *)
  Fixpoint remove_rec (r : rec) (l : list rec) : list rec :=
    match l with
    | [] => []
    | x :: rest => if rec_eqb x r then rest else x :: remove_rec r rest
    end.

  (* Removes the first waiter with the identity key. *)
  Fixpoint remove_wtr (key : nat) (l : list wtr) : list wtr :=
    match l with
    | [] => []
    | x :: rest => if Nat.eqb (w_key x) key then rest else x :: remove_wtr key rest
    end.

  (* A consume fire removes each non-persistent candidate. *)
  Fixpoint remove_candidates (c : list rec) (l : list rec) : list rec :=
    match c with
    | [] => l
    | r :: rest => remove_candidates rest (if r_pers r then l else remove_rec r l)
    end.

  (* The record that a staged store puts into the view. *)
  Definition stored (e : elem) : rec := mkRec (e_bytes e) (e_pers e).

  Definition apply (op : staged) (s : state) : state :=
    match op with
    | SStore e => mkState (stored e :: recs s) (wtrs s)
    | SPark k => mkState (recs s) (k :: wtrs s)
    | SFireOffer _ w _ =>
        mkState (recs s) (if w_pers w then wtrs s else remove_wtr (w_key w) (wtrs s))
    | SFireRecv _ c _ => mkState (remove_candidates c (recs s)) (wtrs s)
    | SRetire r => mkState (remove_rec r (recs s)) (wtrs s)
    end.

  Definition apply_all (ops : list staged) (s : state) : state :=
    fold_left (fun st op => apply op st) ops s.

  (* ── 3. The fire rounds ──────────────────────────────────────────────── *)

  Inductive fire_step : Type := FDone | FContinue | FReissue.

  (* FireRounds::after_fire with the count of fires after this one. *)
  Definition after_fire (fired : nat) (spent stays : bool) : fire_step :=
    if spent then FDone
    else if stays then FReissue
    else if cap <=? fired then FReissue
    else FContinue.

  (* The rest of an operation that a new Intent::Space runs. *)
  Inductive reissue : Type :=
  | ReProduce (e : elem)
  | ReConsume (k : wtr).

  (* ── 4. The scan driver of bag and set (A.7) ─────────────────────────── *)

  (* The first waiter, in the given order, that matches v spatially and
     whose guard admits the binding. *)
  Fixpoint first_admitting (v : V) (ws : list wtr) : option (wtr * M) :=
    match ws with
    | [] => None
    | w :: rest =>
        match spatial (w_pat w) v with
        | Some m => if admits (w_guard w) m then Some (w, m) else first_admitting v rest
        | None => first_admitting v rest
        end
    end.

  Definition in_class (key : K) (r : rec) : bool :=
    match class_key (payload (r_bytes r)) with
    | Some k => if K_eq_dec key k then true else false
    | None => false
    end.

  (* RETIRE-CLASS: a K_retire of each non-persistent stored member of the
     class of v, in the canonical order. A bag has no class. *)
  Definition retire_class (v : V) (l : list rec) : list staged :=
    match class_key v with
    | Some key => map SRetire (filter (fun r => negb (r_pers r) && in_class key r) (data_order l))
    | None => []
    end.

  (* OFFER-SCAN. *)
  Fixpoint offer_loop (fuel : nat) (e : elem) (s : state) (fired : nat)
      : list staged * option reissue :=
    match fuel with
    | O => ([], Some (ReProduce e))
    | S f =>
        match first_admitting (payload (e_bytes e)) (waiter_order (wtrs s)) with
        | None => ([SStore e], None)
        | Some (w, m) =>
            let fire := SFireOffer e w m in
            let s1 := apply fire s in
            let retires := if w_peek w then [] else retire_class (payload (e_bytes e)) (recs s1) in
            match after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w) with
            | FDone => (fire :: retires, None)
            | FReissue => (fire :: retires, Some (ReProduce e))
            | FContinue =>
                let next := offer_loop f e (apply_all retires s1) (S fired) in
                (fire :: retires ++ fst next, snd next)
            end
        end
    end.

  Definition offer_plan (e : elem) (s : state) : list staged * option reissue :=
    offer_loop (S (length (wtrs s))) e s 0.

  (* The first record, in the given order, that matches p spatially. *)
  Fixpoint first_spatial (p : P) (l : list rec) : option (rec * M) :=
    match l with
    | [] => None
    | r :: rest =>
        match spatial p (payload (r_bytes r)) with
        | Some m => Some (r, m)
        | None => first_spatial p rest
        end
    end.

  (* The candidates of a consume fire (F3): r alone for a peek or a bag, and
     otherwise r and the other members of its class in the canonical order. *)
  Definition receive_group (k : wtr) (r : rec) (l : list rec) : list rec :=
    if w_peek k then [r]
    else
      match class_key (payload (r_bytes r)) with
      | None => [r]
      | Some key => r :: filter (fun x => negb (rec_eqb x r) && in_class key x) (data_order l)
      end.

  (* The element of the re-store of a peeked record (Rule E): its own bytes,
     not persistent, with its own authority (Introduction::for_record). *)
  Definition restore_elem (k : wtr) (r : rec) : elem :=
    mkElem (r_bytes r) false (own_auth (r_bytes r)).

  (* RESTORE-SCAN: the record re-offered through the produce plan, with the
     fire rounds of the operation after the fire of the receive (I-21). The
     fire of the receive can be the last fire of the transaction, so a
     second send that meets a full transaction re-issues before it fires. *)
  Definition restore_scan (k : wtr) (r : rec) (s : state) (fired : nat)
      : list staged * option reissue :=
    if cap <=? fired then ([], Some (ReProduce (restore_elem k r))) else
    offer_loop (S (length (wtrs s))) (restore_elem k r) s fired.

  (* RECEIVE-SCAN. *)
  Fixpoint receive_loop (fuel : nat) (k : wtr) (s : state) (fired : nat)
      : list staged * option reissue :=
    match fuel with
    | O => ([], Some (ReConsume k))
    | S f =>
        match first_spatial (w_pat k) (data_order (recs s)) with
        | None => ([SPark k], None)
        | Some (r, m) =>
            if negb (admits (w_guard k) m) then ([SPark k], None)
            else
              let c := receive_group k r (recs s) in
              let fire := SFireRecv k c m in
              let s1 := apply fire s in
              let restored :=
                if w_peek k && negb (r_pers r) then restore_scan k r s1 (S fired) else ([], None) in
              match snd restored with
              | Some re => (fire :: fst restored, Some re)
              | None =>
                  match after_fire (S fired) (negb (w_pers k)) (w_peek k || existsb r_pers c) with
                  | FDone => (fire :: fst restored, None)
                  | FReissue => (fire :: fst restored, Some (ReConsume k))
                  | FContinue =>
                      let next := receive_loop f k (apply_all (fst restored) s1) (S fired) in
                      (fire :: fst restored ++ fst next, snd next)
                  end
              end
        end
    end.

  Definition receive_plan (k : wtr) (s : state) : list staged * option reissue :=
    receive_loop (S (length (recs s))) k s 0.

  (* ── 5. The cell: the head driver with a capacity of one (F15) ───────── *)

  Definition side_full {X : Type} (l : list X) : bool :=
    match l with [] => false | _ :: _ => true end.

  (* offer_head of the cell. None is the CellOccupied abort. *)
  Fixpoint cell_offer_loop (fuel : nat) (e : elem) (s : state) (fired : nat)
      : option (list staged * option reissue) :=
    match fuel with
    | O => Some ([], Some (ReProduce e))
    | S f =>
        let store := if side_full (recs s) then None else Some ([SStore e], None) in
        match wtrs s with
        | [] => store
        | w :: _ =>
            match spatial (w_pat w) (payload (e_bytes e)) with
            | None => store
            | Some m =>
                if negb (admits (w_guard w) m) then store
                else
                  let fire := SFireOffer e w m in
                  match after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w) with
                  | FDone => Some ([fire], None)
                  | FReissue => Some ([fire], Some (ReProduce e))
                  | FContinue =>
                      match cell_offer_loop f e (apply fire s) (S fired) with
                      | Some next => Some (fire :: fst next, snd next)
                      | None => None
                      end
                  end
            end
        end
    end.

  Definition cell_offer_plan (e : elem) (s : state) : option (list staged * option reissue) :=
    cell_offer_loop (S (length (wtrs s))) e s 0.

  (* receive_head of the cell, with the plain restore of a peeked record. *)
  Fixpoint cell_receive_loop (fuel : nat) (k : wtr) (s : state) (fired : nat)
      : option (list staged * option reissue) :=
    match fuel with
    | O => Some ([], Some (ReConsume k))
    | S f =>
        let park := if side_full (wtrs s) then None else Some ([SPark k], None) in
        match recs s with
        | [] => park
        | r :: _ =>
            match spatial (w_pat k) (payload (r_bytes r)) with
            | None => park
            | Some m =>
                if negb (admits (w_guard k) m) then park
                else
                  let fire := SFireRecv k [r] m in
                  let rops := if w_peek k && negb (r_pers r) then [SStore (restore_elem k r)] else [] in
                  match after_fire (S fired) (negb (w_pers k)) (r_pers r || w_peek k) with
                  | FDone => Some (fire :: rops, None)
                  | FReissue => Some (fire :: rops, Some (ReConsume k))
                  | FContinue =>
                      match cell_receive_loop f k (apply_all (fire :: rops) s) (S fired) with
                      | Some next => Some (fire :: rops ++ fst next, snd next)
                      | None => None
                      end
                  end
            end
        end
    end.

  Definition cell_receive_plan (k : wtr) (s : state) : option (list staged * option reissue) :=
    cell_receive_loop (S (length (recs s))) k s 0.

  (* ── 6. The host's second send (Rule E) ──────────────────────────────── *)

  (* produce_peeks: the removed datum, not persistent, with the datum's own
     cost authority. *)
  Definition host_resend (r : rec) : elem := mkElem (r_bytes r) false (own_auth (r_bytes r)).

  (* The second send as the space runs it: the produce plan of host_resend r. *)
  Definition second_send (r : rec) (s : state) (fired : nat) : list staged * option reissue :=
    offer_loop (S (length (wtrs s))) (host_resend r) s fired.

  (* ── 7. Invariants and trace predicates ──────────────────────────────── *)

  (* No pending match: a stored waiter without a guard matches no stored
     record spatially (F22, decision 37). *)
  Definition no_pending (s : state) : Prop :=
    forall w r, In w (wtrs s) -> In r (recs s) -> w_guard w = no_guard ->
      spatial (w_pat w) (payload (r_bytes r)) = None.

  (* No stored pair matches and passes the guard. The cell keeps it. *)
  Definition no_full_match (s : state) : Prop :=
    forall w r m, In w (wtrs s) -> In r (recs s) ->
      spatial (w_pat w) (payload (r_bytes r)) = Some m -> admits (w_guard w) m = false.

  (* The invariant of the cell: no full match, at most one record and at
     most one waiter. *)
  Definition cell_inv (s : state) : Prop :=
    no_full_match s /\ length (recs s) <= 1 /\ length (wtrs s) <= 1.

  (* The facts of one staged operation in the view s before it. *)
  Definition step_ok (s : state) (op : staged) : Prop :=
    match op with
    | SFireOffer e w m =>
        In w (wtrs s) /\ spatial (w_pat w) (payload (e_bytes e)) = Some m /\ admits (w_guard w) m = true
    | SFireRecv k c m =>
        (exists r rest, c = r :: rest /\
           spatial (w_pat k) (payload (r_bytes r)) = Some m /\ admits (w_guard k) m = true) /\
        (forall x, In x c -> In x (recs s))
    | SRetire r => r_pers r = false
    | SStore _ | SPark _ => True
    end.

  (* Every staged operation of a trace satisfies step_ok in its own view. *)
  Fixpoint trace_ok (s : state) (ops : list staged) : Prop :=
    match ops with
    | [] => True
    | op :: rest => step_ok s op /\ trace_ok (apply op s) rest
    end.

  (* ── 8. Lemmas: views ────────────────────────────────────────────────── *)

  Lemma apply_all_app : forall l1 l2 s, apply_all (l1 ++ l2) s = apply_all l2 (apply_all l1 s).
  Proof. intros l1 l2 s. unfold apply_all. rewrite fold_left_app. reflexivity. Qed.

  Lemma apply_all_cons : forall op l s, apply_all (op :: l) s = apply_all l (apply op s).
  Proof. reflexivity. Qed.

  Lemma trace_ok_app :
    forall l1 l2 s, trace_ok s l1 -> trace_ok (apply_all l1 s) l2 -> trace_ok s (l1 ++ l2).
  Proof.
    induction l1 as [| op rest IH]; intros l2 s H1 H2; [exact H2 |].
    destruct H1 as [Hop Hrest]. split; [exact Hop |].
    apply IH; [exact Hrest | exact H2].
  Qed.

  Lemma rec_eqb_true : forall x y, rec_eqb x y = true -> x = y.
  Proof. intros x y H. unfold rec_eqb in H. destruct (rec_eq_dec x y); [assumption | discriminate]. Qed.

  Lemma rec_eqb_refl : forall x, rec_eqb x x = true.
  Proof. intros x. unfold rec_eqb. destruct (rec_eq_dec x x); [reflexivity | contradiction]. Qed.

  Lemma remove_rec_incl : forall r l, incl (remove_rec r l) l.
  Proof.
    intros r l. induction l as [| x rest IH]; [intros y [] |].
    cbn [remove_rec]. destruct (rec_eqb x r).
    - intros y Hy. right. exact Hy.
    - intros y [<- | Hy]; [left; reflexivity | right; exact (IH y Hy)].
  Qed.

  Lemma remove_rec_length : forall r l, length (remove_rec r l) <= length l.
  Proof.
    intros r l. induction l as [| x rest IH]; [cbn; lia |].
    cbn [remove_rec]. destruct (rec_eqb x r); cbn; lia.
  Qed.

  Lemma remove_wtr_incl : forall key l, incl (remove_wtr key l) l.
  Proof.
    intros key l. induction l as [| x rest IH]; [intros y [] |].
    cbn [remove_wtr]. destruct (Nat.eqb (w_key x) key).
    - intros y Hy. right. exact Hy.
    - intros y [<- | Hy]; [left; reflexivity | right; exact (IH y Hy)].
  Qed.

  Lemma remove_wtr_length : forall key l, length (remove_wtr key l) <= length l.
  Proof.
    intros key l. induction l as [| x rest IH]; [cbn; lia |].
    cbn [remove_wtr]. destruct (Nat.eqb (w_key x) key); cbn; lia.
  Qed.

  Lemma remove_candidates_incl : forall c l, incl (remove_candidates c l) l.
  Proof.
    induction c as [| r rest IH]; intros l; [intros y Hy; exact Hy |].
    cbn [remove_candidates]. intros y Hy. apply IH in Hy.
    destruct (r_pers r); [exact Hy | exact (remove_rec_incl r l y Hy)].
  Qed.

  Lemma remove_candidates_length : forall c l, length (remove_candidates c l) <= length l.
  Proof.
    induction c as [| r rest IH]; intros l; [cbn [remove_candidates]; lia |].
    cbn [remove_candidates]. specialize (IH (if r_pers r then l else remove_rec r l)).
    destruct (r_pers r); [exact IH | pose proof (remove_rec_length r l); lia].
  Qed.

  (* A view that keeps a subset of the entries of another view, with no more
     entries on each side. *)
  Definition shrinks (s' s : state) : Prop :=
    incl (recs s') (recs s) /\ incl (wtrs s') (wtrs s) /\
    length (recs s') <= length (recs s) /\ length (wtrs s') <= length (wtrs s).

  Lemma shrinks_refl : forall s, shrinks s s.
  Proof. intros s. split; [apply incl_refl | split; [apply incl_refl | split; lia]]. Qed.

  Lemma shrinks_trans : forall s1 s2 s3, shrinks s1 s2 -> shrinks s2 s3 -> shrinks s1 s3.
  Proof.
    intros s1 s2 s3 (H1 & H2 & H3 & H4) (H5 & H6 & H7 & H8).
    split; [exact (incl_tran H1 H5) | split; [exact (incl_tran H2 H6) | split; lia]].
  Qed.

  (* A removal: a fire or a retirement. *)
  Definition removal (op : staged) : bool :=
    match op with
    | SStore _ | SPark _ => false
    | SFireOffer _ _ _ | SFireRecv _ _ _ | SRetire _ => true
    end.

  Lemma apply_removal_shrinks : forall op s, removal op = true -> shrinks (apply op s) s.
  Proof.
    intros [e | k | e w m | k c m | r] s H; try discriminate; unfold shrinks; cbn [apply recs wtrs].
    - split; [apply incl_refl | split; [| split; [lia |]]].
      + destruct (w_pers w); [apply incl_refl | apply remove_wtr_incl].
      + destruct (w_pers w); [lia | apply remove_wtr_length].
    - split; [apply remove_candidates_incl | split; [apply incl_refl | split; [apply remove_candidates_length | lia]]].
    - split; [apply remove_rec_incl | split; [apply incl_refl | split; [apply remove_rec_length | lia]]].
  Qed.

  Lemma apply_all_removals_shrink :
    forall ops s, forallb removal ops = true -> shrinks (apply_all ops s) s.
  Proof.
    induction ops as [| op rest IH]; intros s H; [apply shrinks_refl |].
    cbn [forallb] in H. apply andb_true_iff in H as [Hop Hrest].
    rewrite apply_all_cons. exact (shrinks_trans _ _ _ (IH _ Hrest) (apply_removal_shrinks op s Hop)).
  Qed.

  Lemma retire_class_removals : forall v l, forallb removal (retire_class v l) = true.
  Proof.
    intros v l. unfold retire_class. destruct (class_key v); [| reflexivity].
    induction (filter _ (data_order l)) as [| r rest IH]; [reflexivity | exact IH].
  Qed.

  Lemma retire_class_trace : forall v l s, trace_ok s (retire_class v l).
  Proof.
    intros v l. unfold retire_class. destruct (class_key v) as [key |]; [| intros s; exact I].
    assert (Hp : forall r, In r (filter (fun r => negb (r_pers r) && in_class key r) (data_order l)) ->
                   r_pers r = false).
    { intros r Hr. apply filter_In in Hr as [_ Hr]. apply andb_true_iff in Hr as [Hr _].
      apply negb_true_iff. exact Hr. }
    induction (filter _ (data_order l)) as [| r rest IH]; intros s; [exact I |].
    split; [exact (Hp r (or_introl eq_refl)) |].
    apply IH. intros x Hx. exact (Hp x (or_intror Hx)).
  Qed.

  Lemma no_pending_shrinks : forall s s', no_pending s -> shrinks s' s -> no_pending s'.
  Proof.
    intros s s' H (Hr & Hw & _) w r Hin_w Hin_r Hg. exact (H w r (Hw w Hin_w) (Hr r Hin_r) Hg).
  Qed.

  Lemma no_full_match_shrinks : forall s s', no_full_match s -> shrinks s' s -> no_full_match s'.
  Proof.
    intros s s' H (Hr & Hw & _) w r m Hin_w Hin_r Hs. exact (H w r m (Hw w Hin_w) (Hr r Hin_r) Hs).
  Qed.

  Lemma cell_inv_shrinks : forall s s', cell_inv s -> shrinks s' s -> cell_inv s'.
  Proof.
    intros s s' (Hf & Hr & Hw) Hsh. split; [exact (no_full_match_shrinks s s' Hf Hsh) |].
    destruct Hsh as (_ & _ & Hlr & Hlw). split; lia.
  Qed.

  Lemma no_full_match_no_pending : forall s, no_full_match s -> no_pending s.
  Proof.
    intros s H w r Hw Hr Hg. destruct (spatial (w_pat w) (payload (r_bytes r))) as [m |] eqn:E;
      [| reflexivity].
    pose proof (H w r m Hw Hr E) as Hf. rewrite Hg, no_guard_admits in Hf. discriminate.
  Qed.

  (* ── 9. Lemmas: the choices of the scan ──────────────────────────────── *)

  Lemma first_admitting_some :
    forall v ws w m, first_admitting v ws = Some (w, m) ->
      In w ws /\ spatial (w_pat w) v = Some m /\ admits (w_guard w) m = true.
  Proof.
    intros v ws. induction ws as [| x rest IH]; intros w m H; [discriminate |].
    cbn [first_admitting] in H.
    destruct (spatial (w_pat x) v) as [m' |] eqn:E.
    - destruct (admits (w_guard x) m') eqn:Ad.
      + injection H as Hw Hm. subst x m'. split; [left; reflexivity | split; assumption].
      + first [discriminate H |
               destruct (IH w m H) as (Hin & Hs & Ha); split; [right; exact Hin | split; assumption]].
    - first [discriminate H |
             destruct (IH w m H) as (Hin & Hs & Ha); split; [right; exact Hin | split; assumption]].
  Qed.

  (* No admitting waiter: every waiter without a guard misses the datum. *)
  Lemma first_admitting_none :
    forall v ws, first_admitting v ws = None ->
      forall w, In w ws -> w_guard w = no_guard -> spatial (w_pat w) v = None.
  Proof.
    intros v ws. induction ws as [| x rest IH]; intros H w Hin Hg; [destruct Hin |].
    cbn [first_admitting] in H.
    destruct Hin as [<- | Hin].
    - destruct (spatial (w_pat x) v) as [m |] eqn:E; [| reflexivity].
      rewrite Hg, no_guard_admits in H. discriminate.
    - refine (IH _ w Hin Hg).
      destruct (spatial (w_pat x) v) as [m |]; [| exact H].
      destruct (admits (w_guard x) m); [discriminate | exact H].
  Qed.

  (* No admitting waiter: every waiter that matches the datum refuses it. *)
  Lemma first_admitting_none_refuses :
    forall v ws, first_admitting v ws = None ->
      forall w m, In w ws -> spatial (w_pat w) v = Some m -> admits (w_guard w) m = false.
  Proof.
    intros v ws. induction ws as [| x rest IH]; intros H w m Hin Hs; [destruct Hin |].
    cbn [first_admitting] in H.
    destruct Hin as [<- | Hin].
    - rewrite Hs in H. destruct (admits (w_guard x) m); [discriminate | reflexivity].
    - refine (IH _ w m Hin Hs).
      destruct (spatial (w_pat x) v) as [m' |]; [| exact H].
      destruct (admits (w_guard x) m'); [discriminate | exact H].
  Qed.

  Lemma first_spatial_some :
    forall p l r m, first_spatial p l = Some (r, m) -> In r l /\ spatial p (payload (r_bytes r)) = Some m.
  Proof.
    intros p l. induction l as [| x rest IH]; intros r m H; [discriminate |].
    cbn [first_spatial] in H. destruct (spatial p (payload (r_bytes x))) as [m' |] eqn:E.
    - injection H as Hr Hm. subst x m'. split; [left; reflexivity | exact E].
    - first [discriminate H | destruct (IH r m H) as [Hin Hs]; split; [right; exact Hin | exact Hs]
            | destruct (first_spatial p rest); discriminate H].
  Qed.

  Lemma first_spatial_none :
    forall p l, first_spatial p l = None -> forall r, In r l -> spatial p (payload (r_bytes r)) = None.
  Proof.
    intros p l. induction l as [| x rest IH]; intros H r Hin; [destruct Hin |].
    cbn [first_spatial] in H. destruct (spatial p (payload (r_bytes x))) as [m |] eqn:E;
      [discriminate |].
    destruct Hin as [<- | Hin]; [exact E | exact (IH H r Hin)].
  Qed.

  Lemma in_data_order : forall l x, In x (data_order l) <-> In x l.
  Proof.
    intros l x. split; intros H.
    - exact (Permutation_in x (data_order_perm l) H).
    - exact (Permutation_in x (Permutation_sym (data_order_perm l)) H).
  Qed.

  Lemma in_waiter_order : forall l x, In x (waiter_order l) <-> In x l.
  Proof.
    intros l x. split; intros H.
    - exact (Permutation_in x (waiter_order_perm l) H).
    - exact (Permutation_in x (Permutation_sym (waiter_order_perm l)) H).
  Qed.

  Lemma receive_group_head :
    forall k r l, exists rest, receive_group k r l = r :: rest.
  Proof.
    intros k r l. unfold receive_group. destruct (w_peek k); [exists []; reflexivity |].
    destruct (class_key (payload (r_bytes r))); [eexists; reflexivity | exists []; reflexivity].
  Qed.

  Lemma receive_group_stored :
    forall k r l, In r l -> forall x, In x (receive_group k r l) -> In x l.
  Proof.
    intros k r l Hr x Hx. unfold receive_group in Hx. destruct (w_peek k).
    - destruct Hx as [<- | []]. exact Hr.
    - destruct (class_key (payload (r_bytes r))) as [key |].
      + destruct Hx as [<- | Hx]; [exact Hr |].
        apply filter_In in Hx as [Hx _]. exact (proj1 (in_data_order l x) Hx).
      + destruct Hx as [<- | []]. exact Hr.
  Qed.

  (* ── 10. Lemmas: Rule E ──────────────────────────────────────────────── *)

  Lemma remove_rec_perm : forall r l, In r l -> Permutation (r :: remove_rec r l) l.
  Proof.
    intros r l. induction l as [| x rest IH]; intros Hin; [destruct Hin |].
    cbn [remove_rec]. destruct (rec_eqb x r) eqn:E.
    - apply rec_eqb_true in E. subst x. reflexivity.
    - destruct Hin as [-> | Hin]; [rewrite rec_eqb_refl in E; discriminate |].
      eapply perm_trans; [apply perm_swap |]. apply perm_skip. exact (IH Hin).
  Qed.

  Lemma scan_peek_resends :
    forall fuel k s r m,
      1 < cap ->
      w_peek k = true -> w_pers k = false -> r_pers r = false ->
      first_spatial (w_pat k) (data_order (recs s)) = Some (r, m) -> admits (w_guard k) m = true ->
      receive_loop (S fuel) k s 0 =
        (SFireRecv k [r] m :: fst (second_send r (apply (SFireRecv k [r] m) s) 1),
         snd (second_send r (apply (SFireRecv k [r] m) s) 1)).
  Proof.
    intros fuel k s r m Hcap Hpk Hpers Hpr F Ad.
    cbn [receive_loop]. rewrite F. rewrite ?Ad. cbn [negb].
    assert (Hc : receive_group k r (recs s) = [r]) by (unfold receive_group; rewrite Hpk; reflexivity).
    rewrite Hc, Hpk, Hpr. cbn [andb negb].
    unfold restore_scan, second_send. fold (host_resend r).
    replace (cap <=? 1) with false by (symmetry; apply Nat.leb_gt; exact Hcap).
    change (restore_elem k r) with (host_resend r).
    destruct (snd (offer_loop (S (length (wtrs (apply (SFireRecv k [r] m) s)))) (host_resend r)
                (apply (SFireRecv k [r] m) s) 1)) as [re |] eqn:E; [reflexivity |].
    rewrite Hpers. cbn [negb]. unfold after_fire. reflexivity.
  Qed.

  Lemma cell_peek_resends :
    forall s k r m,
      cell_inv s -> recs s = [r] -> r_pers r = false -> w_peek k = true -> w_pers k = false ->
      spatial (w_pat k) (payload (r_bytes r)) = Some m -> admits (w_guard k) m = true ->
      cell_offer_plan (host_resend r) (apply (SFireRecv k [r] m) s) = Some ([SStore (host_resend r)], None) /\
      cell_receive_plan k s = Some ([SFireRecv k [r] m; SStore (host_resend r)], None).
  Proof.
    intros s k r m (Hf & Hlr & Hlw) Er Hpr Hpk Hpers Hs Ad. split.
    - unfold cell_offer_plan. cbn [apply recs wtrs remove_candidates].
      rewrite Er, Hpr. cbn [remove_rec]. rewrite rec_eqb_refl.
      cbn [cell_offer_loop recs wtrs side_full].
      destruct (wtrs s) as [| w rest] eqn:Ew; [reflexivity |].
      cbn [host_resend e_bytes].
      destruct (spatial (w_pat w) (payload (r_bytes r))) as [m' |] eqn:Es; [| reflexivity].
      rewrite (Hf w r m' (ltac:(rewrite Ew; left; reflexivity)) (ltac:(rewrite Er; left; reflexivity)) Es).
      reflexivity.
    - unfold cell_receive_plan. rewrite Er. cbn [length cell_receive_loop]. rewrite Er.
      rewrite Hs, Ad. cbn [negb]. rewrite Hpk, Hpr, Hpers. cbn [andb negb orb]. unfold after_fire. cbn.
      reflexivity.
  Qed.

  (* ── 11. Lemmas: the produce plans (F21, F22) ────────────────────────── *)

  Lemma offer_loop_trace :
    forall fuel e s fired, trace_ok s (fst (offer_loop fuel e s fired)).
  Proof.
    induction fuel as [| f IH]; intros e s fired; [exact I |].
    cbn [offer_loop].
    destruct (first_admitting (payload (e_bytes e)) (waiter_order (wtrs s))) as [[w m] |] eqn:F;
      [| split; exact I].
    destruct (first_admitting_some _ _ w m F) as (Hin & Hs & Ha).
    assert (Hstep : step_ok s (SFireOffer e w m)).
    { split; [exact (proj1 (in_waiter_order _ w) Hin) | split; assumption]. }
    set (s1 := apply (SFireOffer e w m) s).
    assert (Hret : trace_ok s1 (if w_peek w then [] else retire_class (payload (e_bytes e)) (recs s1))).
    { destruct (w_peek w); [exact I | apply retire_class_trace]. }
    destruct (after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w));
      cbn [fst]; split; try exact Hstep.
    - exact Hret.
    - apply trace_ok_app; [exact Hret | apply IH].
    - exact Hret.
  Qed.

  Lemma offer_loop_no_pending :
    forall fuel e s fired, no_pending s -> no_pending (apply_all (fst (offer_loop fuel e s fired)) s).
  Proof.
    induction fuel as [| f IH]; intros e s fired H; [exact H |].
    cbn [offer_loop].
    destruct (first_admitting (payload (e_bytes e)) (waiter_order (wtrs s))) as [[w m] |] eqn:F.
    - set (s1 := apply (SFireOffer e w m) s).
      set (retires := if w_peek w then [] else retire_class (payload (e_bytes e)) (recs s1)).
      assert (Hs1 : no_pending (apply_all retires s1)).
      { apply (no_pending_shrinks s); [exact H |].
        apply (shrinks_trans _ s1); [| apply apply_removal_shrinks; reflexivity].
        apply apply_all_removals_shrink. unfold retires.
        destruct (w_peek w); [reflexivity | apply retire_class_removals]. }
      destruct (after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w));
        cbn [fst]; rewrite apply_all_cons; fold s1; try exact Hs1.
      rewrite apply_all_app. apply IH. exact Hs1.
    - cbn [fst apply_all fold_left apply]. intros w r Hw Hr Hg. cbn [recs wtrs] in Hw, Hr.
      destruct Hr as [<- | Hr]; [| exact (H w r Hw Hr Hg)].
      apply (first_admitting_none _ _ F w); [exact (proj2 (in_waiter_order _ w) Hw) | exact Hg].
  Qed.

  (* ── 12. Lemmas: the consume plans (F20, F22) ────────────────────────── *)

  Lemma receive_loop_trace :
    forall fuel k s fired, trace_ok s (fst (receive_loop fuel k s fired)).
  Proof.
    induction fuel as [| f IH]; intros k s fired; [exact I |].
    cbn [receive_loop].
    destruct (first_spatial (w_pat k) (data_order (recs s))) as [[r m] |] eqn:F;
      [| split; exact I].
    destruct (admits (w_guard k) m) eqn:Ad; cbn [negb]; [| split; exact I].
    destruct (first_spatial_some _ _ r m F) as (Hin & Hs).
    apply (proj1 (in_data_order _ r)) in Hin.
    set (c := receive_group k r (recs s)).
    assert (Hstep : step_ok s (SFireRecv k c m)).
    { split.
      - destruct (receive_group_head k r (recs s)) as [rest Hc].
        exists r, rest. split; [exact Hc | split; assumption].
      - exact (receive_group_stored k r (recs s) Hin). }
    set (s1 := apply (SFireRecv k c m) s).
    set (restored := if w_peek k && negb (r_pers r) then restore_scan k r s1 (S fired) else ([], None)).
    assert (Hres : trace_ok s1 (fst restored)).
    { unfold restored, restore_scan. destruct (w_peek k && negb (r_pers r)); [| exact I].
      destruct (cap <=? S fired); [exact I | apply offer_loop_trace]. }
    destruct (snd restored) as [re |]; [cbn [fst]; split; [exact Hstep | exact Hres] |].
    destruct (after_fire (S fired) (negb (w_pers k)) (w_peek k || existsb r_pers c));
      cbn [fst]; split; try exact Hstep; try exact Hres.
    apply trace_ok_app; [exact Hres | apply IH].
  Qed.

  Lemma receive_loop_no_pending :
    forall fuel k s fired, no_pending s -> no_pending (apply_all (fst (receive_loop fuel k s fired)) s).
  Proof.
    induction fuel as [| f IH]; intros k s fired H; [exact H |].
    cbn [receive_loop].
    destruct (first_spatial (w_pat k) (data_order (recs s))) as [[r m] |] eqn:F.
    - destruct (admits (w_guard k) m) eqn:Ad; cbn [negb].
      + set (c := receive_group k r (recs s)).
        set (s1 := apply (SFireRecv k c m) s).
        set (restored := if w_peek k && negb (r_pers r) then restore_scan k r s1 (S fired) else ([], None)).
        assert (Hs1 : no_pending s1).
        { apply (no_pending_shrinks s); [exact H | apply apply_removal_shrinks; reflexivity]. }
        assert (Hs2 : no_pending (apply_all (fst restored) s1)).
        { unfold restored, restore_scan. destruct (w_peek k && negb (r_pers r)); [| exact Hs1].
          destruct (cap <=? S fired); [exact Hs1 |].
          apply offer_loop_no_pending. exact Hs1. }
        destruct (snd restored) as [re |]; [cbn [fst]; rewrite apply_all_cons; exact Hs2 |].
        destruct (after_fire (S fired) (negb (w_pers k)) (w_peek k || existsb r_pers c));
          cbn [fst]; rewrite apply_all_cons; fold s1; try exact Hs2.
        rewrite apply_all_app. apply IH. exact Hs2.
      + cbn [fst apply_all fold_left apply]. intros w r' Hw Hr' Hg. cbn [recs wtrs] in Hw, Hr'.
        destruct Hw as [<- | Hw]; [| exact (H w r' Hw Hr' Hg)].
        rewrite Hg, no_guard_admits in Ad. discriminate.
    - cbn [fst apply_all fold_left apply]. intros w r Hw Hr Hg. cbn [recs wtrs] in Hw, Hr.
      destruct Hw as [<- | Hw]; [| exact (H w r Hw Hr Hg)].
      apply (first_spatial_none _ _ F r). exact (proj2 (in_data_order _ r) Hr).
  Qed.

  (* ── 13. Lemmas: the cell ────────────────────────────────────────────── *)

  Lemma length_le_one_cons :
    forall (X : Type) (x : X) (rest : list X), length (x :: rest) <= 1 -> rest = [].
  Proof. intros X x rest H. destruct rest; [reflexivity | cbn in H; lia]. Qed.

  Lemma cell_offer_loop_ok :
    forall fuel e s fired ops re,
      cell_inv s -> cell_offer_loop fuel e s fired = Some (ops, re) ->
      cell_inv (apply_all ops s) /\ trace_ok s ops.
  Proof.
    induction fuel as [| f IH]; intros e s fired ops re Hinv H.
    { cbn in H. injection H as <- _. split; [exact Hinv | exact I]. }
    cbn [cell_offer_loop] in H.
    assert (Hstore : forall (ops' : list staged) (re' : option reissue),
              (if side_full (recs s) then @None (list staged * option reissue)
               else Some ([SStore e], @None reissue)) = Some (ops', re') ->
              (forall w m, In w (wtrs s) -> spatial (w_pat w) (payload (e_bytes e)) = Some m ->
                           admits (w_guard w) m = false) ->
              cell_inv (apply_all ops' s) /\ trace_ok s ops').
    { intros ops' re' Hst Hrefuse. destruct (recs s) as [| x rest] eqn:Er; cbn [side_full] in Hst;
        [| discriminate].
      injection Hst as <- _. destruct Hinv as (Hf & Hlr & Hlw).
      split; [| split; exact I].
      cbn [apply_all fold_left apply recs wtrs]. rewrite Er.
      split; [| cbn; split; [lia | exact Hlw]].
      intros w r m Hw Hr Hs. destruct Hr as [<- | []]. exact (Hrefuse w m Hw Hs). }
    destruct (wtrs s) as [| w rest] eqn:Ew.
    - apply (Hstore ops re H). intros w m [].
    - assert (Hrest : rest = []) by (destruct Hinv as (_ & _ & Hlw); rewrite Ew in Hlw;
                                      exact (length_le_one_cons _ w rest Hlw)).
      subst rest.
      destruct (spatial (w_pat w) (payload (e_bytes e))) as [m |] eqn:Es.
      + destruct (admits (w_guard w) m) eqn:Ad; cbn [negb] in H.
        * assert (Hstep : step_ok s (SFireOffer e w m)).
          { split; [rewrite Ew; left; reflexivity | split; assumption]. }
          assert (Hsh : shrinks (apply (SFireOffer e w m) s) s) by (apply apply_removal_shrinks; reflexivity).
          destruct (after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w)).
          -- injection H as <- _. split; [exact (cell_inv_shrinks _ _ Hinv Hsh) | split; [exact Hstep | exact I]].
          -- destruct (cell_offer_loop f e (apply (SFireOffer e w m) s) (S fired)) as [[ops' re'] |] eqn:En;
               [| discriminate].
             injection H as <- _. cbn [fst].
             destruct (IH e _ (S fired) ops' re' (cell_inv_shrinks _ _ Hinv Hsh) En) as [Hinv' Htr].
             split; [rewrite apply_all_cons; exact Hinv' | split; [exact Hstep | exact Htr]].
          -- injection H as <- _. split; [exact (cell_inv_shrinks _ _ Hinv Hsh) | split; [exact Hstep | exact I]].
        * apply (Hstore ops re H). intros w' m' [<- | []] Hs'. rewrite Es in Hs'.
          injection Hs' as <-. exact Ad.
      + apply (Hstore ops re H). intros w' m' [<- | []] Hs'. rewrite Es in Hs'. discriminate.
  Qed.

  Lemma restore_elem_stored : forall k r, r_pers r = false -> stored (restore_elem k r) = r.
  Proof. intros k [b p] Hp. cbn in Hp. subst p. reflexivity. Qed.

  (* The view of a cell after a consume fire of its record r and the plain
     restore of a peek. *)
  Lemma cell_receive_fire_state :
    forall s k r m, recs s = [r] ->
      apply_all (SFireRecv k [r] m ::
                 (if w_peek k && negb (r_pers r) then [SStore (restore_elem k r)] else [])) s =
      mkState (if r_pers r then [r] else if w_peek k then [r] else []) (wtrs s).
  Proof.
    intros s k r m Er.
    destruct (r_pers r) eqn:Ep; destruct (w_peek k); cbn [negb andb];
      cbn [apply_all fold_left apply recs wtrs remove_candidates]; rewrite Er, Ep; cbn;
      try rewrite rec_eqb_refl; try rewrite (restore_elem_stored k r Ep); reflexivity.
  Qed.

  Lemma cell_receive_loop_ok :
    forall fuel k s fired ops re,
      cell_inv s -> cell_receive_loop fuel k s fired = Some (ops, re) ->
      cell_inv (apply_all ops s) /\ trace_ok s ops.
  Proof.
    induction fuel as [| f IH]; intros k s fired ops re Hinv H.
    { cbn in H. injection H as <- _. split; [exact Hinv | exact I]. }
    cbn [cell_receive_loop] in H.
    assert (Hpark : forall (ops' : list staged) (re' : option reissue),
              (if side_full (wtrs s) then @None (list staged * option reissue)
               else Some ([SPark k], @None reissue)) = Some (ops', re') ->
              (forall r m, In r (recs s) -> spatial (w_pat k) (payload (r_bytes r)) = Some m ->
                           admits (w_guard k) m = false) ->
              cell_inv (apply_all ops' s) /\ trace_ok s ops').
    { intros ops' re' Hp Hrefuse. destruct (wtrs s) as [| x rest] eqn:Ew; cbn [side_full] in Hp;
        [| discriminate].
      injection Hp as <- _. destruct Hinv as (Hf & Hlr & Hlw).
      split; [| split; exact I].
      cbn [apply_all fold_left apply recs wtrs]. rewrite Ew.
      split; [| cbn; split; [exact Hlr | lia]].
      intros w r m Hw Hr Hs. destruct Hw as [<- | []]. exact (Hrefuse r m Hr Hs). }
    destruct (recs s) as [| r rest] eqn:Er.
    - apply (Hpark ops re H). intros r m [].
    - assert (Hrest : rest = []) by (destruct Hinv as (_ & Hlr & _); rewrite Er in Hlr;
                                      exact (length_le_one_cons _ r rest Hlr)).
      subst rest.
      destruct (spatial (w_pat k) (payload (r_bytes r))) as [m |] eqn:Es.
      + destruct (admits (w_guard k) m) eqn:Ad; cbn [negb] in H.
        * set (rops := if w_peek k && negb (r_pers r) then [SStore (restore_elem k r)] else []).
          fold rops in H.
          assert (Hstep : step_ok s (SFireRecv k [r] m)).
          { split; [exists r, []; split; [reflexivity | split; assumption] |].
            intros x [<- | []]. rewrite Er. left; reflexivity. }
          assert (Hrops : trace_ok (apply (SFireRecv k [r] m) s) rops).
          { unfold rops. destruct (w_peek k && negb (r_pers r)); [split; exact I | exact I]. }
          (* After the fire and the restore, the cell holds what it held, or nothing. *)
          assert (Hafter : cell_inv (apply_all (SFireRecv k [r] m :: rops) s)).
          { unfold rops. rewrite (cell_receive_fire_state s k r m Er).
            destruct Hinv as (Hf & Hlr & Hlw).
            split; [| cbn [recs wtrs]; split; [destruct (r_pers r), (w_peek k); cbn; lia | exact Hlw]].
            intros w x m' Hw Hx Hs'. cbn [recs wtrs] in Hw, Hx.
            apply (Hf w x m' Hw); [| exact Hs'].
            rewrite Er. destruct (r_pers r), (w_peek k); cbn in Hx; first [exact Hx | destruct Hx]. }
          destruct (after_fire (S fired) (negb (w_pers k)) (r_pers r || w_peek k)).
          -- injection H as <- _. split; [exact Hafter | split; [exact Hstep | exact Hrops]].
          -- destruct (cell_receive_loop f k (apply_all (SFireRecv k [r] m :: rops) s) (S fired))
               as [[ops' re'] |] eqn:En; [| discriminate].
             injection H as <- _. cbn [fst].
             destruct (IH k _ (S fired) ops' re' Hafter En) as [Hinv' Htr].
             split.
             ++ change (SFireRecv k [r] m :: rops ++ ops') with ((SFireRecv k [r] m :: rops) ++ ops').
                rewrite apply_all_app. exact Hinv'.
             ++ split; [exact Hstep |]. apply trace_ok_app; [exact Hrops | exact Htr].
          -- injection H as <- _. split; [exact Hafter | split; [exact Hstep | exact Hrops]].
        * apply (Hpark ops re H). intros r' m' [<- | []] Hs'. rewrite Es in Hs'.
          injection Hs' as <-. exact Ad.
      + apply (Hpark ops re H). intros r' m' [<- | []] Hs'. rewrite Es in Hs'. discriminate.
  Qed.

  (* ── 14. Headlines ───────────────────────────────────────────────────── *)

  (* F20: every consume plan, of the scan driver and of the cell, stages only
     fires whose dispatched record is a stored record that matches the
     pattern spatially and whose binding the guard admitted. Every candidate
     of a group fire is stored, every retirement retires a non-persistent
     record, and the re-sends of the plan are produce plans (F21). *)
  Theorem consume_fires_only_matching :
    (forall k s, trace_ok s (fst (receive_plan k s))) /\
    (forall fuel k s fired, trace_ok s (fst (receive_loop fuel k s fired))) /\
    (forall k s ops re, cell_inv s -> cell_receive_plan k s = Some (ops, re) -> trace_ok s ops).
  Proof.
    split; [intros k s; apply receive_loop_trace |].
    split; [exact receive_loop_trace |].
    intros k s ops re Hinv H. exact (proj2 (cell_receive_loop_ok _ k s 0 ops re Hinv H)).
  Qed.

  (* F21: every produce plan, of the scan driver and of the cell, stages only
     fires whose waiter is stored, matches the datum spatially and admits
     the binding. *)
  Theorem produce_fires_only_matching :
    (forall e s, trace_ok s (fst (offer_plan e s))) /\
    (forall fuel e s fired, trace_ok s (fst (offer_loop fuel e s fired))) /\
    (forall e s ops re, cell_inv s -> cell_offer_plan e s = Some (ops, re) -> trace_ok s ops).
  Proof.
    split; [intros e s; apply offer_loop_trace |].
    split; [exact offer_loop_trace |].
    intros e s ops re Hinv H. exact (proj2 (cell_offer_loop_ok _ e s 0 ops re Hinv H)).
  Qed.

  (* F22: bag, set and cell plans keep the rule of no pending match. Every
     produce, consume and re-entry of the scan driver keeps no_pending, every
     committed plan of the cell keeps cell_inv, and cell_inv implies
     no_pending. So a waiter without a guard never meets a stored record that
     matches it spatially (FIPS:97-107, restated for decision 37). *)
  Theorem unordered_no_pending_match :
    (forall fuel e s fired, no_pending s -> no_pending (apply_all (fst (offer_loop fuel e s fired)) s)) /\
    (forall fuel k s fired, no_pending s -> no_pending (apply_all (fst (receive_loop fuel k s fired)) s)) /\
    (forall fuel e s fired ops re, cell_inv s -> cell_offer_loop fuel e s fired = Some (ops, re) ->
       cell_inv (apply_all ops s)) /\
    (forall fuel k s fired ops re, cell_inv s -> cell_receive_loop fuel k s fired = Some (ops, re) ->
       cell_inv (apply_all ops s)) /\
    (forall s, cell_inv s -> no_pending s) /\
    (forall s w r, no_pending s -> In w (wtrs s) -> In r (recs s) -> w_guard w = no_guard ->
       spatial (w_pat w) (payload (r_bytes r)) = None).
  Proof.
    split; [exact offer_loop_no_pending |].
    split; [exact receive_loop_no_pending |].
    split; [intros fuel e s fired ops re Hinv H; exact (proj1 (cell_offer_loop_ok fuel e s fired ops re Hinv H)) |].
    split; [intros fuel k s fired ops re Hinv H; exact (proj1 (cell_receive_loop_ok fuel k s fired ops re Hinv H)) |].
    split; [intros s [Hf _]; exact (no_full_match_no_pending s Hf) |].
    intros s w r H Hw Hr Hg. exact (H w r Hw Hr Hg).
  Qed.

  (* Rule E (decision 45): the re-store of a peek is the second send of the
     host.
     1. Scan driver: a peek receive that fires a non-persistent record r
        stages the fire over [r] and then exactly the plan of the second
        send of r on the view after the fire, with that fire counted (I-21),
        for a cap of two fires or more (FIRE_ROUND_CAP is 64).
     2. The re-sent element is the host's: the bytes of r, not persistent,
        the record's own authority, and so the identity of r.
     3. When the second send stores r, the records after the peek are the
        records before, up to the order, and the waiters are unchanged: no
        record is lost and none is duplicated.
     4. Cell: the plain restore of a peek is the second send of the cell, so
        the cell never reports CellOccupied for it. *)
  Theorem peek_restore_equals_host_reproduce :
    (forall fuel k s r m,
       1 < cap ->
       w_peek k = true -> w_pers k = false -> r_pers r = false ->
       first_spatial (w_pat k) (data_order (recs s)) = Some (r, m) -> admits (w_guard k) m = true ->
       receive_loop (S fuel) k s 0 =
         (SFireRecv k [r] m :: fst (second_send r (apply (SFireRecv k [r] m) s) 1),
          snd (second_send r (apply (SFireRecv k [r] m) s) 1))) /\
    (forall r, r_pers r = false ->
       e_bytes (host_resend r) = r_bytes r /\ e_pers (host_resend r) = false /\
       e_auth (host_resend r) = own_auth (r_bytes r) /\ elem_ident (host_resend r) = rec_ident r) /\
    (forall s k r m, In r (recs s) -> r_pers r = false ->
       Permutation (recs (apply_all [SFireRecv k [r] m; SStore (host_resend r)] s)) (recs s) /\
       wtrs (apply_all [SFireRecv k [r] m; SStore (host_resend r)] s) = wtrs s) /\
    (forall s k r m,
       cell_inv s -> recs s = [r] -> r_pers r = false -> w_peek k = true -> w_pers k = false ->
       spatial (w_pat k) (payload (r_bytes r)) = Some m -> admits (w_guard k) m = true ->
       exists ss, cell_offer_plan (host_resend r) (apply (SFireRecv k [r] m) s) = Some ss /\
                  cell_receive_plan k s = Some (SFireRecv k [r] m :: fst ss, snd ss)).
  Proof.
    split; [exact scan_peek_resends |].
    split.
    { intros [b p] Hp. cbn in Hp |- *. subst p. unfold elem_ident, rec_ident. cbn.
      split; [reflexivity | split; [reflexivity | split; reflexivity]]. }
    split.
    { intros s k [b p] m Hin Hp. cbn in Hp. subst p.
      cbn [apply_all fold_left apply recs wtrs remove_candidates r_pers]. split; [| reflexivity].
      unfold stored, host_resend. cbn [e_bytes e_pers r_bytes].
      exact (remove_rec_perm (mkRec b false) (recs s) Hin). }
    intros s k r m Hinv Er Hpr Hpk Hpers Hs Ad.
    destruct (cell_peek_resends s k r m Hinv Er Hpr Hpk Hpers Hs Ad) as [Hoffer Hrecv].
    exists ([SStore (host_resend r)], None). split; [exact Hoffer | exact Hrecv].
  Qed.

  (* ── 15. Re-issues of a collection without a counter (decision 43) ────── *)

  (* operation_ord: the order of a round from the re-issued order and the
     bump. None is the Inconsistent error. *)
  Definition operation_ord (reissued : option nat) (bumped : option nat) : option nat :=
    match reissued, bumped with
    | None, Some b => Some b
    | None, None => Some 0
    | Some o, Some b => if o <? b then Some o else None
    | Some o, None => if o =? 0 then Some 0 else None
    end.

  Variable D : Type.                    (* the datum of a send: ListParWithRandom *)
  Variable wrap : nat -> D -> B.        (* record::wrap_datum with the order *)
  Variable unwrap : B -> option (nat * D).   (* record::unwrap_datum *)
  Hypothesis wrap_unwrap : forall b o d, unwrap b = Some (o, d) -> wrap o d = b.

  (* A produce operation: its datum, its persistence, its introduction
     authority and its re-issued order. *)
  Record produce_op : Type := mkProduceOp {
    op_data : D; op_pers : bool; op_auth : A; op_reissued : option nat
  }.

  (* The element that a round of a scan produce stages: the record of the
     order of the round, with no counter. *)
  Definition scan_round (op : produce_op) : option elem :=
    match operation_ord (op_reissued op) None with
    | Some o => Some (mkElem (wrap o (op_data op)) (op_pers op) (op_auth op))
    | None => None
    end.

  (* A.7: a re-issued scan produce is ProduceOp(x) with reissued_ord = None. *)
  Definition scan_reissue (op : produce_op) : produce_op :=
    mkProduceOp (op_data op) (op_pers op) (op_auth op) None.

  (* A.7: at the cap, RESTORE-SCAN re-issues ProduceOp(unwrap(r)), not
     persistent, with the record's own authority. *)
  Definition restore_reissue (r : rec) : option produce_op :=
    match unwrap (r_bytes r) with
    | Some (_, d) => Some (mkProduceOp d false (own_auth (r_bytes r)) None)
    | None => None
    end.

  Lemma offer_loop_reissue_shape :
    forall fuel e s fired x, snd (offer_loop fuel e s fired) = Some x -> x = ReProduce e.
  Proof.
    induction fuel as [| f IH]; intros e s fired x H.
    { cbn in H. injection H as <-. reflexivity. }
    cbn [offer_loop] in H.
    destruct (first_admitting (payload (e_bytes e)) (waiter_order (wtrs s))) as [[w m] |];
      [| discriminate].
    destruct (after_fire (S fired) (negb (e_pers e) && negb (w_peek w)) (w_pers w)); cbn [snd] in H.
    - discriminate.
    - exact (IH _ _ _ _ H).
    - injection H as <-. reflexivity.
  Qed.

  Lemma receive_loop_reissue :
    forall fuel k s fired x, snd (receive_loop fuel k s fired) = Some x ->
      x = ReConsume k \/ exists r, x = ReProduce (restore_elem k r) /\ r_pers r = false.
  Proof.
    induction fuel as [| f IH]; intros k s fired x H.
    { cbn in H. injection H as <-. left; reflexivity. }
    cbn [receive_loop] in H.
    destruct (first_spatial (w_pat k) (data_order (recs s))) as [[r m] |]; [| discriminate].
    destruct (admits (w_guard k) m); cbn [negb] in H; [| discriminate].
    set (c := receive_group k r (recs s)) in H.
    set (s1 := apply (SFireRecv k c m) s) in H.
    destruct (w_peek k && negb (r_pers r)) eqn:Er.
    - apply andb_true_iff in Er as [_ Hpr]. apply negb_true_iff in Hpr.
      unfold restore_scan in H.
      destruct (cap <=? S fired) eqn:Ecap.
      + cbn [snd] in H. injection H as <-. right. exists r. split; [reflexivity | exact Hpr].
      + destruct (snd (offer_loop (S (length (wtrs s1))) (restore_elem k r) s1 (S fired)))
          as [re |] eqn:E.
        * cbn [snd] in H. injection H as <-. right. exists r. split; [| exact Hpr].
          exact (offer_loop_reissue_shape _ _ _ _ re E).
        * destruct (after_fire (S fired) (negb (w_pers k)) (w_peek k || existsb r_pers c));
            cbn [snd] in H.
          -- discriminate.
          -- exact (IH _ _ _ _ H).
          -- injection H as <-. left; reflexivity.
    - cbn [snd] in H.
      destruct (after_fire (S fired) (negb (w_pers k)) (w_peek k || existsb r_pers c)); cbn [snd] in H.
      + discriminate.
      + exact (IH _ _ _ _ H).
      + injection H as <-. left; reflexivity.
  Qed.

  (* A re-issue of a scan produce that carries no order is the same
     operation, in every round. *)
  Lemma scan_reissue_iter :
    forall n op, op_reissued op = None -> Nat.iter n scan_reissue op = op.
  Proof.
    induction n as [| n IH]; intros op H; [reflexivity |].
    rewrite Nat.iter_succ, (IH op H).
    destruct op as [d p a r]. cbn in H. subst r. reflexivity.
  Qed.

  (* Decision 43 for a collection without a counter (bag and set).
     1. Every order is 0, and a re-issued order other than 0 is
        Inconsistent.
     2. Every round of a re-issued scan produce stages the element of its
        first round: the same record bytes, persistence and authority, so
        the same produce identity.
     3. A produce plan re-issues its own element, and a consume plan
        re-issues itself or the re-send of a peeked non-persistent record.
     4. The cap re-issue of a re-send, ProduceOp(unwrap(r)), stages in its
        next round the record r itself, not persistent, with its own
        authority: the element of the re-send, with the identity of r. *)
  Theorem scan_reissue_identity_stable :
    (forall reissued, operation_ord reissued None =
       match reissued with None => Some 0 | Some o => if o =? 0 then Some 0 else None end) /\
    (forall op n e, op_reissued op = None -> scan_round op = Some e ->
       scan_round (Nat.iter n scan_reissue op) = Some e /\
       elem_ident e = ident (wrap 0 (op_data op)) (op_pers op)) /\
    (forall fuel e s fired x, snd (offer_loop fuel e s fired) = Some x -> x = ReProduce e) /\
    (forall fuel k s fired x, snd (receive_loop fuel k s fired) = Some x ->
       x = ReConsume k \/ exists r, x = ReProduce (restore_elem k r) /\ r_pers r = false) /\
    (forall k r d op e, r_pers r = false -> unwrap (r_bytes r) = Some (0, d) ->
       restore_reissue r = Some op -> scan_round op = Some e ->
       e = restore_elem k r /\ elem_ident e = rec_ident r).
  Proof.
    split; [intros [o |]; reflexivity |].
    split.
    { intros op n e Hr He. unfold scan_round in He. rewrite Hr in He. cbn in He.
      injection He as <-. split.
      - rewrite (scan_reissue_iter n op Hr). unfold scan_round. rewrite Hr. reflexivity.
      - reflexivity. }
    split; [exact offer_loop_reissue_shape |].
    split; [exact receive_loop_reissue |].
    intros k [b p] d op e Hp Hu Hop He. cbn in Hp, Hu. subst p.
    unfold restore_reissue in Hop. cbn [r_bytes] in Hop. rewrite Hu in Hop. injection Hop as <-.
    unfold scan_round in He. cbn in He. injection He as <-.
    rewrite (wrap_unwrap b 0 d Hu). unfold restore_elem, elem_ident, rec_ident. cbn.
    split; reflexivity.
  Qed.
End Engine.

Print Assumptions consume_fires_only_matching.
Print Assumptions produce_fires_only_matching.
Print Assumptions unordered_no_pending_match.
Print Assumptions peek_restore_equals_host_reproduce.
Print Assumptions scan_reissue_identity_stable.
