(* ═══════════════════════════════════════════════════════════════════════════
   VectorDbDiscipline.v — the VectorDB discipline of one collection (P6)

   This module models the driver of a VectorDB collection: RECEIVE-VDB,
   OFFER-VDB and the TAIL with LIST-CASCADE, as one transaction of a space
   job. It proves five claims:
   - a removal exposes no join waiter whose binds are in single mode and
     that has no guard, because the similarity window of a bind commutes
     with every removal (the analog of slots_cons_filter of JoinCascade.v,
     with its ref_fill_mono restated here);
   - a transaction that ends leaves no pending match for a waiter without a
     guard, in single mode and in list mode (LM-b, plan decision 66);
   - the list cascade is needed: a removal can make a parked list waiter
     without a guard fireable (the counterexample CE-2 of the arbitration);
   - a transaction fires at most FIRE_ROUND_CAP times, and its loops end
     within a bound that the plans compute;
   - Rule E (decision 45) loses no record: a record of the pre-state is in
     the post-state of a transaction that ends, or a fire of a waiter that
     does not peek spent it.

   Sources
   - Design: .reified-rspaces-work/p6/design-v3.md, sections 4.3 to 4.6
     and 5 (the Rocq VectorDB discipline).
   - Plan: decisions 30 (a datum that a fire does not spend is offered
     again), 37 (one guard check), 45 (Rule E), 49 (one fire count for each
     transaction), 50 (the cascade of a joined set, the precedent of
     LIST-CASCADE), 66 (LM-b) and I-21 (FIRE_ROUND_CAP).
   - Code: this repository, P6 step 8. The line numbers below are its
     lines.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   rec, wtr, vst            rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1239-1252 (Element: a record, its
                            source and its persistence), the records and the
                            waiters of the staged view of a collection
   elig, esort              rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:881-952 (eligible: the score, the
                            bound and the ids, then the selection order of
                            tensor/exact.rs, Similarity.v S13 to S15)
   sel, fires               rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1637-1689, 1068-1117
                            (receive_selection, list_selection)
   osel, ofires             rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1484-1532, 1123-1142
                            (first_offer_target, list_offer)
   after_fire               rholang/src/rust/interpreter/reified/costs.rs:
                            322-333 (FireRounds::after_fire)
   retired                  rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1151-1194 (retire_members)
   offer                    rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1422-1471 (offer_vdb)
   receive                  rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1546-1626 (receive_plan)
   first_cascade, tail      rholang/src/rust/interpreter/spaces/disciplines/
                            vectordb.rs:1773-1808, 1860-1902, 1909-1954
                            (run_tail, first_cascade_target,
                            lift_and_reoffer)
   jfill, first_fit         rholang/src/rust/interpreter/spaces/join.rs:
                            3055-3168, 3321-3389, 1669-1764 (search: the
                            greedy search of the engine; similarity_bind:
                            the first record of the selection order of a
                            bind that its pattern binds, without the claimed
                            records; window: JoinWindow::Similarity)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - One collection: its records (a list) and its parked waiters (a list in
     canonical order). A VectorDB collection has no counter (M-1). A store
     into a full collection fails with CapacityExceeded and aborts the
     deploy, so the job commits nothing, and the model has no capacity.
   - A query is abstract: its eligibility elig and its selection order
     esort (the eligible records in the order of tensor/exact.rs: the better
     score, then kappa). The hypotheses on esort are the facts that
     Similarity.v proves for that order (S13 to S15): a permutation of the
     eligible records, a function of the multiset of the records, and an
     insertion that keeps the order of the other records. The corollary
     vdb_parameters_valid discharges them with an insertion sort by a total
     order of the records.
   - A waiter binds in single mode (the first record of its order whose
     value its pattern binds: binds1) or in list mode (the first k records
     of its order, when its pattern binds their list: bindsL). A receive
     without a query is in single mode, with a query that admits every
     record in the canonical order.
   - The guard is one check over the bound values (decision 37): admits. A
     waiter without a guard admits every binding (free_admits). The claims
     are for the waiters without a guard.
   - The search of OFFER-VDB starts at the first waiter. The code starts at
     the fired waiter when no record left (offer_vdb, tested = position).
     The waiters before it failed for the same records and the same datum,
     so both searches find the same waiter.
   - A stop (the round cap or a divergent pair) ends the transaction of the
     model. The code then re-issues a settle job or the operation, and a
     later transaction goes on. The claims are for transactions that end
     without a stop, as for the stack (StackDiscipline.v headlines 2 and 3).
   - t_spent logs the ids that a fire of a waiter that does not peek spends:
     the stored members that a fire consumes or retires, and the datum of an
     offer that the fire uses up.
   - Each loop has fuel. The plans compute their fuel from the state, and
     vdb_txn_bounded shows that no loop runs out of it.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. vdb_join_removal_exposes_nothing  a removal keeps the order of each
                                          similarity window, so a greedy
                                          search of single-mode binds that
                                          fails before a removal fails
                                          after it; one window is the
                                          selection of the driver
    2. vdb_no_pending_match              a transaction that ends leaves no
                                          waiter without a guard that can
                                          fire (single mode and LM-b)
    3. vdb_list_needs_cascade            a computed negative result: without
                                          LIST-CASCADE a receive ends with a
                                          fireable list waiter (CE-2)
    4. vdb_txn_bounded                   a transaction fires at most cap
                                          times, and its loops end within
                                          the fuel that the plans compute
    5. vdb_no_record_lost                Rule E: a record of the pre-state
                                          stays, or a fire of a waiter that
                                          does not peek spent it

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - The Rocq 9.1 standard library has no lemma firstn_In. Derive
     In x (firstn n l) -> In x l from firstn_skipn (in_firstn).
   - destruct (firstn k l) eqn:El also rewrites the hypotheses that mention
     firstn k l. In receive_nl the hypothesis Hx then states the fact for
     the empty list, so the first branch must use it: intros _; discriminate
     Hx.
   - The first statement of TI_drop had the premise out = []. A fire
     removes records, so that premise fails at every fire. The premise
     out = [] \/ (t_rm t || f) = true covers the parks and the fires.
   - apply tail_inv cannot unify the fuel of its conclusion with the goal.
     Give the transactions: exact (tail_inv _ t1 t H1 Ht).
   - apply (jfill_mono keep ...) cannot infer the claimed list C, because C
     occurs only under jfill. Use refine with every argument
     (vdb_join_removal_exposes_nothing).
   - lia does not compute length [] or length (x :: rest), and it does not
     see through a set variable. Use cbn [length] after the case split of
     the owed list, and unfold t0 before lia (tail_bounded).
   - tail_inv first asserted TI for the next transaction, written out in
     full, and applied the induction hypothesis to that assertion. The
     control vdb-lift-not-owed changes that transaction, so it broke
     tail_inv, whose statement stays true. Apply the hypothesis to the
     transaction of the goal, and prove TI there (TI_drop_set).
   - A control that drops the claims of first_fit keeps every statement of
     this module true: without claims first_fit is find, and find fails on
     a filtered list when it fails on the list. The control vdb-window-top1
     cuts the similarity window instead, which makes the removal claim of
     headline 1 false.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── 1. Lists ──────────────────────────────────────────────────────────── *)

Definition is_nil {A : Type} (l : list A) : bool := match l with [] => true | _ => false end.

Lemma find_filter_none :
  forall (A : Type) (p f : A -> bool) l, find p l = None -> find p (filter f l) = None.
Proof.
  intros A p f l. induction l as [| x l IH]; intros H; [reflexivity |].
  cbn [find] in H. destruct (p x) eqn:Ep; [discriminate H |].
  cbn [filter]. destruct (f x); [cbn [find]; rewrite Ep |]; exact (IH H).
Qed.

Lemma find_skip :
  forall (A : Type) (p : A -> bool) l1 x l2, p x = false -> find p (l1 ++ x :: l2) = find p (l1 ++ l2).
Proof.
  intros A p l1 x l2 Hx. induction l1 as [| y l1 IH]; cbn [app find]; [rewrite Hx; reflexivity |].
  destruct (p y); [reflexivity | exact IH].
Qed.

Lemma find_in : forall (A : Type) (p : A -> bool) l x, find p l = Some x -> In x l.
Proof.
  intros A p l x. induction l as [| y l IH]; intros H; [discriminate H |].
  cbn [find] in H. destruct (p y); [injection H as <-; left; reflexivity | right; exact (IH H)].
Qed.

Lemma in_firstn : forall (A : Type) n (l : list A) x, In x (firstn n l) -> In x l.
Proof. intros A n l x H. rewrite <- (firstn_skipn n l). apply in_or_app. left. exact H. Qed.

Lemma firstn_insert_late :
  forall (A : Type) k (l1 l2 : list A) x, k <= length l1 -> firstn k (l1 ++ x :: l2) = firstn k (l1 ++ l2).
Proof.
  intros A k l1 l2 x H. rewrite !firstn_app. replace (k - length l1) with 0 by lia. reflexivity.
Qed.

Lemma firstn_insert_early :
  forall (A : Type) k (l1 l2 : list A) x, length l1 < k -> In x (firstn k (l1 ++ x :: l2)).
Proof.
  intros A k l1 l2 x H. rewrite firstn_app. apply in_or_app. right.
  replace (k - length l1) with (S (k - length l1 - 1)) by lia. cbn [firstn]. left. reflexivity.
Qed.

(* ── 2. The model ────────────────────────────────────────────────────── *)

(* The mode of a waiter: single, or a list of the first k eligible records. *)
Inductive mode : Type := Single | Listed (k : nat).

(* The end of a plan: it ended, it stopped (a re-issue), or it ran out of
   fuel. *)
Inductive outcome : Type := Ended | Stopped | NoFuel.

(* FireStep of FireRounds::after_fire. *)
Inductive step : Type := Done | Cont | Stop.

Section VectorDb.
  Variable V : Type.     (* the value of a record that a pattern binds *)
  Variable Qy : Type.    (* a query: its vector, metric, bound and ids *)
  Variable P : Type.     (* a pattern *)
  Variable cap : nat.    (* FIRE_ROUND_CAP *)

  (* A record: its id (the hash of its source produce, stable across Rule E
     and the re-issues of decision 43), its value and its persistence. *)
  Record rec : Type := mkRec { r_id : nat; r_val : V; r_pers : bool }.

  (* The eligible records of a query in its selection order. *)
  Variable elig : Qy -> rec -> bool.
  Variable esort : Qy -> list rec -> list rec.
  Hypothesis esort_perm : forall q l, Permutation (esort q l) (filter (elig q) l).
  Hypothesis esort_set : forall q l l', Permutation l l' -> esort q l = esort q l'.
  Hypothesis esort_insert :
    forall q x l, exists l1 l2, esort q l = l1 ++ l2 /\
      esort q (x :: l) = (if elig q x then l1 ++ x :: l2 else l1 ++ l2).

  (* The spatial match of a single-mode pattern, and of a list-mode pattern
     on the list of the values. *)
  Variable binds1 : P -> V -> bool.
  Variable bindsL : P -> list V -> bool.

  (* A waiter: its id, its query, its pattern, its mode, whether it has no
     guard, and its flags. *)
  Record wtr : Type := mkWtr {
    w_id : nat; w_q : Qy; w_pat : P; w_mode : mode; w_free : bool; w_pers : bool; w_peek : bool }.

  (* The guard: one check over the bound values (decision 37). *)
  Variable admits : wtr -> list V -> bool.
  Hypothesis free_admits : forall W vs, w_free W = true -> admits W vs = true.

  (* ── 3. The window of a query commutes with every removal ──────────── *)

  Lemma esort_filter : forall q f l, esort q (filter f l) = filter f (esort q l).
  Proof.
    intros q f l. remember (length (filter (fun e => negb (f e)) l)) as n eqn:En. revert l En.
    induction n as [| n IH]; intros l En.
    - assert (Hall : forall e, In e l -> f e = true).
      { intros e He. destruct (f e) eqn:Ef; [reflexivity | exfalso].
        assert (Hin : In e (filter (fun e => negb (f e)) l))
          by (apply filter_In; split; [exact He | rewrite Ef; reflexivity]).
        symmetry in En. apply length_zero_iff_nil in En. rewrite En in Hin. exact Hin. }
      rewrite (forallb_filter_id f l) by (apply forallb_forall; exact Hall).
      symmetry. apply forallb_filter_id. apply forallb_forall. intros e He. apply Hall.
      apply (Permutation_in _ (esort_perm q l)) in He. apply filter_In in He as [He _]. exact He.
    - assert (Hex : exists d, In d (filter (fun e => negb (f e)) l)).
      { destruct (filter (fun e => negb (f e)) l) as [| d rest]; [discriminate | exists d; left; reflexivity]. }
      destruct Hex as [d Hd]. apply filter_In in Hd as [Hd Hfd]. apply negb_true_iff in Hfd.
      apply in_split in Hd as (a & b & ->).
      assert (Hn : n = length (filter (fun e => negb (f e)) (a ++ b))).
      { rewrite filter_app, length_app. rewrite filter_app, length_app in En. cbn [filter] in En.
        rewrite Hfd in En. cbn [negb length] in En. lia. }
      destruct (esort_insert q d (a ++ b)) as (l1 & l2 & E1 & E2).
      assert (Ea : filter f (a ++ d :: b) = filter f (a ++ b))
        by (rewrite !filter_app; cbn [filter]; rewrite Hfd; reflexivity).
      assert (Ep : esort q (a ++ d :: b) = esort q (d :: a ++ b)).
      { apply esort_set. apply Permutation_sym. apply Permutation_middle. }
      rewrite Ea, Ep, E2, (IH (a ++ b) Hn), E1.
      destruct (elig q d); rewrite !filter_app; cbn [filter]; rewrite ?Hfd; reflexivity.
  Qed.

  Lemma esort_in : forall q l x, In x (esort q l) -> In x l /\ elig q x = true.
  Proof.
    intros q l x H. apply (Permutation_in _ (esort_perm q l)) in H. apply filter_In in H. exact H.
  Qed.

  (* ── 4. Selections ──────────────────────────────────────────────────── *)

  Definition elist (W : wtr) (R : list rec) : list rec := esort (w_q W) R.

  (* RECEIVE-VDB: the members of a fire of W on the records R. *)
  Definition sel (W : wtr) (R : list rec) : list rec :=
    match w_mode W with
    | Single =>
        match find (fun r => binds1 (w_pat W) (r_val r)) (elist W R) with Some r => [r] | None => [] end
    | Listed k =>
        if bindsL (w_pat W) (map r_val (firstn k (elist W R))) then firstn k (elist W R) else []
    end.

  Definition fires (W : wtr) (R : list rec) : bool :=
    negb (is_nil (sel W R)) && admits W (map r_val (sel W R)).

  Definition has_id (i : nat) (L : list rec) : bool := existsb (fun r => r_id r =? i) L.

  (* OFFER-VDB: the members of a fire of W with the incoming record x. *)
  Definition osel (W : wtr) (R : list rec) (x : rec) : list rec :=
    match w_mode W with
    | Single => if elig (w_q W) x && binds1 (w_pat W) (r_val x) then [x] else []
    | Listed k =>
        if has_id (r_id x) (firstn k (elist W (x :: R))) &&
           bindsL (w_pat W) (map r_val (firstn k (elist W (x :: R))))
        then firstn k (elist W (x :: R)) else []
    end.

  Definition ofires (W : wtr) (R : list rec) (x : rec) : bool :=
    negb (is_nil (osel W R x)) && admits W (map r_val (osel W R x)).

  Lemma free_fires : forall W R, w_free W = true -> fires W R = negb (is_nil (sel W R)).
  Proof. intros W R H. unfold fires. rewrite (free_admits W _ H), andb_true_r. reflexivity. Qed.

  Lemma free_ofires : forall W R x, w_free W = true -> ofires W R x = negb (is_nil (osel W R x)).
  Proof. intros W R x H. unfold ofires. rewrite (free_admits W _ H), andb_true_r. reflexivity. Qed.

  Lemma sel_in : forall W R r, In r (sel W R) -> In r R.
  Proof.
    intros W R r H. unfold sel in H. destruct (w_mode W) as [| k].
    - destruct (find _ (elist W R)) as [r0 |] eqn:E; [| destruct H].
      destruct H as [<- | []]. exact (proj1 (esort_in _ _ _ (find_in _ _ _ _ E))).
    - destruct (bindsL _ _); [| destruct H]. apply in_firstn in H. exact (proj1 (esort_in _ _ _ H)).
  Qed.

  (* A removal never makes a single-mode waiter without a guard fireable. *)
  Lemma single_filter :
    forall W R f, w_mode W = Single -> w_free W = true -> fires W R = false -> fires W (filter f R) = false.
  Proof.
    intros W R f Hm Hf H. rewrite free_fires in H |- * by exact Hf.
    unfold sel, elist in H |- *. rewrite Hm in H |- *. rewrite esort_filter.
    destruct (find (fun r => binds1 (w_pat W) (r_val r)) (esort (w_q W) R)) eqn:E; [discriminate H |].
    rewrite (find_filter_none _ _ _ _ E). reflexivity.
  Qed.

  (* A store of x that fires no waiter makes no waiter without a guard
     fireable: x is not eligible, or it does not bind, or it falls behind
     the first k records of a list. *)
  Lemma store_keeps :
    forall W R x, w_free W = true -> fires W R = false -> ofires W R x = false -> fires W (x :: R) = false.
  Proof.
    intros W R x Hf H Ho. rewrite free_fires in H |- * by exact Hf. rewrite free_ofires in Ho by exact Hf.
    unfold sel, osel, elist in *. destruct (esort_insert (w_q W) x R) as (l1 & l2 & E1 & E2).
    rewrite E2. rewrite E1 in H. rewrite E2 in Ho.
    destruct (w_mode W) as [| k].
    - destruct (elig (w_q W) x) eqn:Ee; [| exact H].
      destruct (binds1 (w_pat W) (r_val x)) eqn:Eb; [discriminate Ho |].
      rewrite (find_skip _ _ l1 x l2 Eb). exact H.
    - destruct (elig (w_q W) x) eqn:Ee; [| exact H].
      destruct (Nat.le_gt_cases k (length l1)) as [Hk | Hk].
      + rewrite (firstn_insert_late _ k l1 l2 x Hk). exact H.
      + assert (Hx : has_id (r_id x) (firstn k (l1 ++ x :: l2)) = true).
        { unfold has_id. apply existsb_exists. exists x. split; [exact (firstn_insert_early _ k l1 l2 x Hk) | apply Nat.eqb_refl]. }
        rewrite Hx in Ho. cbn [andb] in Ho.
        destruct (bindsL (w_pat W) (map r_val (firstn k (l1 ++ x :: l2)))) eqn:Eb; [| reflexivity].
        exfalso. revert Ho. destruct (firstn k (l1 ++ x :: l2)) eqn:El; [intros _; discriminate Hx | discriminate].
  Qed.

  (* ── 5. The state and the plans ──────────────────────────────────────── *)

  Record vst : Type := mkSt { recs : list rec; wtrs : list wtr }.

  (* A transaction: the staged state, the fire count (one for each
     transaction, decision 49), the owed second sends (Rule E), whether a
     record left since the last cascade search, and the spent ids. *)
  Record tx : Type := mkTx { t_st : vst; t_fired : nat; t_owed : list rec; t_rm : bool; t_spent : list nat }.

  Definition fresh (s : vst) : tx := mkTx s 0 [] false [].

  (* The records without the ids of out. *)
  Definition drop (out : list rec) (R : list rec) : list rec := filter (fun r => negb (has_id (r_id r) out)) R.

  (* A fired waiter leaves its place unless it is persistent. *)
  Definition after_w (W : wtr) (ws : list wtr) : list wtr :=
    if w_pers W then ws else filter (fun W' => negb (w_id W' =? w_id W)) ws.

  (* retire_members: the stored members that a fire of W with x retires,
     the members other than x that are not persistent, unless W peeks. *)
  Definition retired (W : wtr) (L : list rec) (x : rec) : list rec :=
    if w_peek W then [] else filter (fun r => negb (r_pers r) && negb (r_id r =? r_id x)) L.

  (* FireRounds::after_fire with the count after the fire. *)
  Definition after_fire (fired : nat) (spent stays : bool) : step :=
    if spent then Done else if stays then Stop else if cap <=? fired then Stop else Cont.

  Fixpoint first_offer (R : list rec) (x : rec) (ws : list wtr) : option wtr :=
    match ws with
    | [] => None
    | W :: rest => if ofires W R x then Some W else first_offer R x rest
    end.

  (* OFFER-VDB of x: x is stored, a fire uses it up, a fire stops the
     transaction, or x is offered again after a fire (decision 30). *)
  Fixpoint offer (fuel : nat) (t : tx) (x : rec) : tx * outcome :=
    match fuel with
    | 0 => (t, NoFuel)
    | S fuel' =>
        match first_offer (recs (t_st t)) x (wtrs (t_st t)) with
        | None => (mkTx (mkSt (x :: recs (t_st t)) (wtrs (t_st t))) (t_fired t) (t_owed t) (t_rm t) (t_spent t), Ended)
        | Some W =>
            let out := retired W (osel W (recs (t_st t)) x) x in
            let spent := negb (r_pers x) && negb (w_peek W) in
            let t' := mkTx (mkSt (drop out (recs (t_st t))) (after_w W (wtrs (t_st t)))) (S (t_fired t))
                           (t_owed t) (t_rm t || negb (is_nil out))
                           (map r_id out ++ (if spent then [r_id x] else []) ++ t_spent t) in
            match after_fire (S (t_fired t)) spent (w_pers W) with
            | Done => (t', Ended)
            | Stop => (t', Stopped)
            | Cont => offer fuel' t' x
            end
        end
    end.

  (* RECEIVE-VDB of W: W fires on its members and the COMM removes the
     non-persistent ones, which a peek owes as second sends (Rule E). A
     persistent W that stays fires again. Otherwise W parks. *)
  Fixpoint receive (fuel : nat) (t : tx) (W : wtr) : tx * outcome :=
    match fuel with
    | 0 => (t, NoFuel)
    | S fuel' =>
        if fires W (recs (t_st t)) then
          let gone := filter (fun r => negb (r_pers r)) (sel W (recs (t_st t))) in
          let t' := mkTx (mkSt (drop gone (recs (t_st t))) (wtrs (t_st t))) (S (t_fired t))
                         (if w_peek W then gone ++ t_owed t else t_owed t) true
                         (if w_peek W then t_spent t else map r_id gone ++ t_spent t) in
          match after_fire (S (t_fired t)) (negb (w_pers W)) (w_peek W || existsb r_pers (sel W (recs (t_st t)))) with
          | Done => (t', Ended)
          | Stop => (t', Stopped)
          | Cont => receive fuel' t' W
          end
        else (mkTx (mkSt (recs (t_st t)) (W :: wtrs (t_st t))) (t_fired t) (t_owed t) (t_rm t) (t_spent t), Ended)
    end.

  Definition is_list (W : wtr) : bool := match w_mode W with Listed _ => true | Single => false end.

  (* first_cascade_target: the first parked list waiter that fires. *)
  Fixpoint first_cascade (R : list rec) (ws : list wtr) : option wtr :=
    match ws with
    | [] => None
    | W :: rest => if is_list W && fires W R then Some W else first_cascade R rest
    end.

  Definition offer_fuel (t : tx) : nat := S (length (wtrs (t_st t))).

  (* TAIL: the owed second sends first, then LIST-CASCADE, until no parked
     list waiter fires. A cascade fire lifts its first member (K_lift when
     it is not persistent), fires the waiter with it, and retires the other
     non-persistent members unless the waiter peeks. A lifted member that a
     peek did not spend is owed again (decision 30). *)
  Fixpoint tail (fuel : nat) (t : tx) : tx * outcome :=
    match fuel with
    | 0 => (t, NoFuel)
    | S fuel' =>
        match t_owed t with
        | x :: rest =>
            if cap <=? t_fired t then (t, Stopped) else
            let t0 := mkTx (t_st t) (t_fired t) rest (t_rm t) (t_spent t) in
            match offer (offer_fuel t0) t0 x with
            | (t', Ended) => tail fuel' t'
            | r => r
            end
        | [] =>
            if negb (t_rm t) then (t, Ended) else
            match first_cascade (recs (t_st t)) (wtrs (t_st t)) with
            | None => (mkTx (t_st t) (t_fired t) [] false (t_spent t), Ended)
            | Some W =>
                if cap <=? t_fired t then (t, Stopped) else
                match sel W (recs (t_st t)) with
                | [] => (t, Stopped)
                | lead :: others =>
                    let lifted := negb (r_pers lead) in
                    let ret := if w_peek W then [] else filter (fun r => negb (r_pers r)) others in
                    let out := (if lifted then [lead] else []) ++ ret in
                    let spent := lifted && negb (w_peek W) in
                    let t' := mkTx (mkSt (drop out (recs (t_st t))) (after_w W (wtrs (t_st t)))) (S (t_fired t))
                                   (if w_peek W && lifted then [lead] else []) true
                                   (map r_id ret ++ (if spent then [r_id lead] else []) ++ t_spent t) in
                    match after_fire (S (t_fired t)) spent (w_pers W) with
                    | Stop => (t', Stopped)
                    | _ => tail fuel' t'
                    end
                end
            end
        end
    end.

  Definition receive_fuel (t : tx) : nat := S (length (recs (t_st t))).
  Definition tail_fuel (t : tx) : nat := S (2 * (cap - t_fired t) + length (t_owed t)).

  (* One transaction: the operation, then the tail. *)
  Definition run_receive (s : vst) (W : wtr) : tx * outcome :=
    match receive (receive_fuel (fresh s)) (fresh s) W with
    | (t, Ended) => tail (tail_fuel t) t
    | r => r
    end.

  Definition run_offer (s : vst) (x : rec) : tx * outcome :=
    match offer (offer_fuel (fresh s)) (fresh s) x with
    | (t, Ended) => tail (tail_fuel t) t
    | r => r
    end.

  (* ── 6. No pending match (LM-b) ──────────────────────────────────────── *)

  (* INV: no parked waiter without a guard can fire. *)
  Definition INV (s : vst) : Prop :=
    forall W, In W (wtrs s) -> w_free W = true -> fires W (recs s) = false.

  (* The invariant inside a transaction: a single-mode waiter without a
     guard never fires, and a list waiter without a guard does not fire
     while no record left since the last cascade search. *)
  Definition TI (t : tx) : Prop :=
    forall W, In W (wtrs (t_st t)) -> w_free W = true -> (w_mode W = Single \/ t_rm t = false) ->
      fires W (recs (t_st t)) = false.

  Lemma TI_fresh : forall s, INV s -> TI (fresh s).
  Proof. intros s H W HW Hf _. exact (H W HW Hf). Qed.

  Lemma drop_nil : forall R, drop [] R = R.
  Proof. intros R. unfold drop. apply forallb_filter_id. apply forallb_forall. intros r _. reflexivity. Qed.

  Lemma after_w_incl : forall W ws, incl (after_w W ws) ws.
  Proof.
    intros W ws W' H. unfold after_w in H. destruct (w_pers W); [exact H |]. apply filter_In in H as [H _]. exact H.
  Qed.

  (* A removal and fewer waiters keep TI, and a removal sets the flag. *)
  Lemma TI_drop :
    forall t out ws' f n owed sp,
      TI t -> incl ws' (wtrs (t_st t)) -> (out = [] \/ (t_rm t || f) = true) ->
      TI (mkTx (mkSt (drop out (recs (t_st t))) ws') n owed (t_rm t || f) sp).
  Proof.
    intros t out ws' f n owed sp H Hi Hfl W HW Hf Hm. cbn [t_st recs wtrs t_rm] in *.
    destruct Hm as [Hm | Hm].
    - unfold drop. apply (single_filter W _ _ Hm Hf). exact (H W (Hi W HW) Hf (or_introl Hm)).
    - destruct Hfl as [-> | Hfl]; [| rewrite Hm in Hfl; discriminate Hfl].
      apply orb_false_iff in Hm as [Hr _]. rewrite drop_nil. exact (H W (Hi W HW) Hf (or_intror Hr)).
  Qed.

  (* TI_drop when a record left: the flag is set. *)
  Lemma TI_drop_set :
    forall t out ws' n owed sp, TI t -> incl ws' (wtrs (t_st t)) ->
      TI (mkTx (mkSt (drop out (recs (t_st t))) ws') n owed true sp).
  Proof.
    intros t out ws' n owed sp H Hi.
    pose proof (TI_drop t out ws' true n owed sp H Hi (or_intror (orb_true_r _))) as Hd.
    rewrite orb_true_r in Hd. exact Hd.
  Qed.

  Lemma first_offer_none :
    forall R x ws, first_offer R x ws = None -> forall W, In W ws -> ofires W R x = false.
  Proof.
    intros R x ws. induction ws as [| W0 rest IH]; intros H W HW; [destruct HW |].
    cbn [first_offer] in H. destruct (ofires W0 R x) eqn:E; [discriminate H |].
    destruct HW as [<- | HW]; [exact E | exact (IH H W HW)].
  Qed.

  Lemma first_offer_in : forall R x ws W, first_offer R x ws = Some W -> In W ws.
  Proof.
    intros R x ws. induction ws as [| W0 rest IH]; intros W H; [discriminate H |].
    cbn [first_offer] in H. destruct (ofires W0 R x); [injection H as <-; left; reflexivity | right; exact (IH W H)].
  Qed.

  Lemma TI_store :
    forall t x, TI t -> first_offer (recs (t_st t)) x (wtrs (t_st t)) = None ->
      TI (mkTx (mkSt (x :: recs (t_st t)) (wtrs (t_st t))) (t_fired t) (t_owed t) (t_rm t) (t_spent t)).
  Proof.
    intros t x H Hn W HW Hf Hm. cbn [t_st recs wtrs t_rm] in *.
    apply (store_keeps W _ x Hf (H W HW Hf Hm)). exact (first_offer_none _ _ _ Hn W HW).
  Qed.

  Lemma TI_offer : forall fuel t x, TI t -> TI (fst (offer fuel t x)).
  Proof.
    induction fuel as [| fuel IH]; intros t x H; [exact H |]. cbn [offer].
    destruct (first_offer (recs (t_st t)) x (wtrs (t_st t))) as [W |] eqn:E; [| exact (TI_store t x H E)].
    assert (Hd : TI (mkTx (mkSt (drop (retired W (osel W (recs (t_st t)) x) x) (recs (t_st t)))
                              (after_w W (wtrs (t_st t)))) (S (t_fired t)) (t_owed t)
                         (t_rm t || negb (is_nil (retired W (osel W (recs (t_st t)) x) x)))
                         (map r_id (retired W (osel W (recs (t_st t)) x) x) ++
                          (if negb (r_pers x) && negb (w_peek W) then [r_id x] else []) ++ t_spent t))).
    { apply TI_drop; [exact H | apply after_w_incl |].
      destruct (retired W (osel W (recs (t_st t)) x) x) as [| o os]; [left; reflexivity |].
      right. apply orb_true_r. }
    destruct (after_fire _ _ _); [exact Hd | apply IH; exact Hd | exact Hd].
  Qed.

  Lemma TI_park :
    forall t W, TI t -> fires W (recs (t_st t)) = false ->
      TI (mkTx (mkSt (recs (t_st t)) (W :: wtrs (t_st t))) (t_fired t) (t_owed t) (t_rm t) (t_spent t)).
  Proof.
    intros t W H Hw W' HW Hf Hm. cbn [t_st recs wtrs t_rm] in *.
    destruct HW as [<- | HW]; [exact Hw | exact (H W' HW Hf Hm)].
  Qed.

  Lemma TI_receive : forall fuel t W, TI t -> TI (fst (receive fuel t W)).
  Proof.
    induction fuel as [| fuel IH]; intros t W H; [exact H |]. cbn [receive].
    destruct (fires W (recs (t_st t))) eqn:E; [| exact (TI_park t W H E)].
    set (gone := filter (fun r => negb (r_pers r)) (sel W (recs (t_st t)))).
    assert (Hd : TI (mkTx (mkSt (drop gone (recs (t_st t))) (wtrs (t_st t))) (S (t_fired t))
                         (if w_peek W then gone ++ t_owed t else t_owed t) (t_rm t || true)
                         (if w_peek W then t_spent t else map r_id gone ++ t_spent t))).
    { apply TI_drop; [exact H | apply incl_refl | right; apply orb_true_r]. }
    rewrite orb_true_r in Hd.
    destruct (after_fire _ _ _); [exact Hd | apply IH; exact Hd | exact Hd].
  Qed.

  Lemma first_cascade_none :
    forall R ws, first_cascade R ws = None -> forall W, In W ws -> is_list W = true -> fires W R = false.
  Proof.
    intros R ws. induction ws as [| W0 rest IH]; intros H W HW Hl; [destruct HW |].
    cbn [first_cascade] in H. destruct (is_list W0 && fires W0 R) eqn:E; [discriminate H |].
    destruct HW as [<- | HW]; [| exact (IH H W HW Hl)].
    rewrite Hl in E. exact E.
  Qed.

  Lemma TI_INV_end :
    forall t, TI t -> (t_rm t = false \/ first_cascade (recs (t_st t)) (wtrs (t_st t)) = None) -> INV (t_st t).
  Proof.
    intros t H Hend W HW Hf. destruct (w_mode W) as [| k] eqn:Hm.
    - exact (H W HW Hf (or_introl Hm)).
    - destruct Hend as [Hr | Hn]; [exact (H W HW Hf (or_intror Hr)) |].
      apply (first_cascade_none _ _ Hn W HW). unfold is_list. rewrite Hm. reflexivity.
  Qed.

  Lemma tail_inv :
    forall fuel t t', TI t -> tail fuel t = (t', Ended) -> INV (t_st t').
  Proof.
    induction fuel as [| fuel IH]; intros t t' H Ht; [discriminate Ht |]. cbn [tail] in Ht.
    destruct (t_owed t) as [| x rest] eqn:Eo.
    - destruct (t_rm t) eqn:Er.
      + cbn [negb] in Ht. destruct (first_cascade (recs (t_st t)) (wtrs (t_st t))) as [W |] eqn:Ec.
        * destruct (cap <=? t_fired t); [discriminate Ht |].
          destruct (sel W (recs (t_st t))) as [| lead others] eqn:Es; [discriminate Ht |].
          destruct (after_fire _ _ _); [refine (IH _ _ _ Ht) | refine (IH _ _ _ Ht) | discriminate Ht];
            (apply TI_drop_set; [exact H | apply after_w_incl]).
        * injection Ht as <-. apply (TI_INV_end (mkTx (t_st t) (t_fired t) [] false (t_spent t))); [| left; reflexivity].
          intros W HW Hf _. destruct (w_mode W) as [| k] eqn:Hm.
          -- exact (H W HW Hf (or_introl Hm)).
          -- apply (first_cascade_none _ _ Ec W HW). unfold is_list. rewrite Hm. reflexivity.
      + cbn [negb] in Ht. injection Ht as <-. exact (TI_INV_end t H (or_introl Er)).
    - destruct (cap <=? t_fired t); [discriminate Ht |].
      destruct (offer _ _ x) as [t1 o] eqn:Eoff.
      assert (H1 : TI t1).
      { replace t1 with (fst (offer (offer_fuel (mkTx (t_st t) (t_fired t) rest (t_rm t) (t_spent t)))
                                       (mkTx (t_st t) (t_fired t) rest (t_rm t) (t_spent t)) x))
          by (rewrite Eoff; reflexivity).
        apply TI_offer. intros W HW Hf Hm. exact (H W HW Hf Hm). }
      destruct o; [exact (IH _ _ H1 Ht) | discriminate Ht | discriminate Ht].
  Qed.

  (* Headline 2 (LM-b, decision 66). A transaction that ends leaves no
     waiter without a guard that can fire: a receive fires or parks, an
     offer stores its datum or fires the first waiter that it fires, and
     the tail offers the owed second sends and runs LIST-CASCADE until no
     parked list waiter fires. A single-mode waiter needs no cascade: a
     removal never makes it fireable (single_filter). *)
  Theorem vdb_no_pending_match :
    forall s W x, INV s ->
      (forall t, run_receive s W = (t, Ended) -> INV (t_st t)) /\
      (forall t, run_offer s x = (t, Ended) -> INV (t_st t)).
  Proof.
    intros s W x Hs. split.
    - intros t Ht. unfold run_receive in Ht.
      destruct (receive (receive_fuel (fresh s)) (fresh s) W) as [t1 o] eqn:E.
      destruct o; [| discriminate Ht | discriminate Ht].
      assert (H1 : TI t1).
      { replace t1 with (fst (receive (receive_fuel (fresh s)) (fresh s) W)) by (rewrite E; reflexivity).
        apply TI_receive. exact (TI_fresh s Hs). }
      exact (tail_inv _ t1 t H1 Ht).
    - intros t Ht. unfold run_offer in Ht.
      destruct (offer (offer_fuel (fresh s)) (fresh s) x) as [t1 o] eqn:E.
      destruct o; [| discriminate Ht | discriminate Ht].
      assert (H1 : TI t1).
      { replace t1 with (fst (offer (offer_fuel (fresh s)) (fresh s) x)) by (rewrite E; reflexivity).
        apply TI_offer. exact (TI_fresh s Hs). }
      exact (tail_inv _ t1 t H1 Ht).
  Qed.

  (* ── 7. Joins of single-mode binds: the similarity windows ─────────── *)

  (* The claim of a record: its id. A record that an earlier bind of the
     same fire claimed is not a candidate of a later bind. *)
  Definition claimed (C : list nat) (e : rec) : bool := existsb (Nat.eqb (r_id e)) C.

  (* The first record of a window that no earlier bind claimed and whose
     value the pattern binds. *)
  Fixpoint first_fit (C : list nat) (p : P) (L : list rec) : option rec :=
    match L with
    | [] => None
    | e :: more =>
        if claimed C e then first_fit C p more
        else if binds1 p (r_val e) then Some e else first_fit C p more
    end.

  (* GREEDY of the join engine over the binds in group order: each bind
     picks from its own similarity window (JoinWindow::Similarity: the
     eligible records of its query in its selection order) and claims the
     record. *)
  Fixpoint jfill (C : list nat) (bs : list (Qy * P)) (R : list rec) : option (list rec) :=
    match bs with
    | [] => Some []
    | (q, p) :: rest =>
        match first_fit C p (esort q R) with
        | Some e => option_map (cons e) (jfill (r_id e :: C) rest R)
        | None => None
        end
    end.

  (* After a removal, a pick fails if it failed before, under claims that
     hold the claims before or removed records (JoinCascade.v
     first_fit_mono, over the ids). *)
  Lemma first_fit_mono :
    forall (keep : rec -> bool) C C' p L,
      (forall e, claimed C e = true -> claimed C' e = true \/ keep e = false) ->
      match first_fit C p L with
      | None => first_fit C' p (filter keep L) = None
      | Some e =>
          match first_fit C' p (filter keep L) with
          | None => True
          | Some e' => keep e = false \/ claimed C' e = true \/ e' = e
          end
      end.
  Proof.
    intros keep C C' p L Hc. induction L as [| e0 more IH]; [reflexivity |].
    cbn [first_fit filter]. destruct (keep e0) eqn:Ek.
    - cbn [first_fit]. destruct (claimed C e0) eqn:E1.
      + destruct (Hc e0 E1) as [E2 | E2]; [rewrite E2; exact IH | rewrite Ek in E2; discriminate].
      + destruct (binds1 p (r_val e0)) eqn:Es.
        * destruct (claimed C' e0) eqn:E2.
          -- destruct (first_fit C' p (filter keep more)) as [e' |]; [right; left; reflexivity | exact I].
          -- right. right. reflexivity.
        * destruct (claimed C' e0); exact IH.
    - destruct (claimed C e0); [exact IH |].
      destruct (binds1 p (r_val e0)); [| exact IH].
      destruct (first_fit C' p (filter keep more)) as [e' |]; [left; exact Ek | exact I].
  Qed.

  (* JoinCascade.v ref_fill_mono over the similarity windows: a greedy
     search that fails before a removal fails after it. The windows after a
     removal are the windows before it, filtered (esort_filter). *)
  Lemma jfill_mono :
    forall (keep : rec -> bool), (forall e e', r_id e = r_id e' -> keep e = keep e') ->
      forall bs C C' R,
        (forall e, claimed C e = true -> claimed C' e = true \/ keep e = false) ->
        jfill C bs R = None -> jfill C' bs (filter keep R) = None.
  Proof.
    intros keep Hk bs. induction bs as [| [q p] rest IH]; intros C C' R Hc H; [discriminate H |].
    cbn [jfill] in H |- *. rewrite esort_filter.
    pose proof (first_fit_mono keep C C' p (esort q R) Hc) as Hm.
    destruct (first_fit C p (esort q R)) as [e |] eqn:E1; [| rewrite Hm; reflexivity].
    destruct (first_fit C' p (filter keep (esort q R))) as [e' |] eqn:E2; [| reflexivity].
    destruct (jfill (r_id e :: C) rest R) as [sg |] eqn:E3; [discriminate H |].
    rewrite (IH (r_id e :: C) (r_id e' :: C')); [reflexivity | | exact E3].
    intros e0 H0. unfold claimed in H0 |- *. cbn [existsb] in H0 |- *.
    apply orb_true_iff in H0 as [H0 | H0].
    - apply Nat.eqb_eq in H0. destruct Hm as [Hm | [Hm | Hm]].
      + right. rewrite (Hk e0 e H0). exact Hm.
      + left. apply orb_true_iff. right. unfold claimed in Hm. rewrite H0. exact Hm.
      + left. apply orb_true_iff. left. apply Nat.eqb_eq. rewrite Hm. exact H0.
    - destruct (Hc e0 H0) as [H1 | H1]; [left; apply orb_true_iff; right; exact H1 | right; exact H1].
  Qed.

  Lemma first_fit_nil : forall p L, first_fit [] p L = find (fun r => binds1 p (r_val r)) L.
  Proof.
    intros p L. induction L as [| e L IH]; [reflexivity |].
    cbn [first_fit find claimed existsb]. destruct (binds1 p (r_val e)); [reflexivity | exact IH].
  Qed.

  (* Headline 1 (design v3 4.6, the similarity analog of slots_cons_filter of
     JoinCascade.v). A removal keeps the order of each similarity window: the
     window after a removal is the window before it, filtered. So a greedy
     search of single-mode binds that fails before a removal fails after it
     (ref_fill_mono), and a removal exposes no join waiter in single mode
     without a guard: such joins need no cascade. The search of one bind is
     the selection of the driver, so the engine and the driver agree. *)
  Theorem vdb_join_removal_exposes_nothing :
    (forall q f R, esort q (filter f R) = filter f (esort q R)) /\
    (forall bs R out, jfill [] bs R = None -> jfill [] bs (drop out R) = None) /\
    (forall W R, w_mode W = Single ->
       jfill [] [(w_q W, w_pat W)] R =
       option_map (fun r => [r]) (find (fun r => binds1 (w_pat W) (r_val r)) (elist W R))).
  Proof.
    split; [exact esort_filter |]. split.
    - intros bs R out H. unfold drop.
      refine (jfill_mono (fun r => negb (has_id (r_id r) out)) _ bs [] [] R _ H).
      + intros e e' E. rewrite E. reflexivity.
      + intros e He. left. exact He.
    - intros W R _. cbn [jfill]. rewrite first_fit_nil. unfold elist.
      destruct (find _ (esort (w_q W) R)); reflexivity.
  Qed.

  (* ── 8. Bounds ──────────────────────────────────────────────────────── *)

  Lemma filter_length_lt :
    forall (A : Type) (f : A -> bool) l x, In x l -> f x = false -> length (filter f l) < length l.
  Proof.
    intros A f l x. induction l as [| y l IH]; intros Hx Hf; [destruct Hx |].
    cbn [filter length]. destruct Hx as [<- | Hx].
    - rewrite Hf. pose proof (filter_length_le f l). lia.
    - destruct (f y); cbn [length]; [specialize (IH Hx Hf); lia | pose proof (filter_length_le f l); lia].
  Qed.

  Lemma after_fire_cont : forall n sp st, after_fire n sp st = Cont -> sp = false /\ st = false /\ n < cap.
  Proof.
    intros n sp st H. unfold after_fire in H.
    destruct sp; [discriminate H |]. destruct st; [discriminate H |].
    destruct (cap <=? n) eqn:E; [discriminate H |]. apply Nat.leb_gt in E. split; [reflexivity | split; [reflexivity | exact E]].
  Qed.

  Lemma after_fire_done : forall n sp st, after_fire n sp st = Done -> sp = true.
  Proof.
    intros n sp st H. unfold after_fire in H. destruct sp; [reflexivity |].
    destruct st; [discriminate H |]. destruct (cap <=? n); discriminate H.
  Qed.

  Lemma after_w_shrinks : forall W ws, In W ws -> w_pers W = false -> length (after_w W ws) < length ws.
  Proof.
    intros W ws HW Hp. unfold after_w. rewrite Hp.
    apply (filter_length_lt _ _ ws W HW). rewrite Nat.eqb_refl. reflexivity.
  Qed.

  Lemma drop_shrinks : forall out R r, In r out -> In r R -> length (drop out R) < length R.
  Proof.
    intros out R r Ho HR. unfold drop. apply (filter_length_lt _ _ R r HR).
    apply negb_false_iff. unfold has_id. apply existsb_exists. exists r. split; [exact Ho | apply Nat.eqb_refl].
  Qed.

  Lemma offer_bounded :
    forall n t x, length (wtrs (t_st t)) < n -> t_fired t < cap ->
      snd (offer n t x) <> NoFuel /\ t_fired (fst (offer n t x)) <= cap /\
      t_fired t <= t_fired (fst (offer n t x)) /\ t_owed (fst (offer n t x)) = t_owed t.
  Proof.
    induction n as [| n IH]; intros t x Hn Hc; [lia |]. cbn [offer].
    destruct (first_offer (recs (t_st t)) x (wtrs (t_st t))) as [W |] eqn:E.
    - destruct (after_fire (S (t_fired t)) (negb (r_pers x) && negb (w_peek W)) (w_pers W)) eqn:Ea.
      + cbn [fst snd t_fired t_owed]. repeat split; [discriminate | lia | lia].
      + destruct (after_fire_cont _ _ _ Ea) as (_ & Hp & Hlt).
        match goal with
        | |- context [offer n ?t' x] =>
            assert (Hw : length (wtrs (t_st t')) < n)
              by (cbn [t_st wtrs]; pose proof (after_w_shrinks W _ (first_offer_in _ _ _ _ E) Hp); lia);
            assert (Hf : t_fired t' < cap) by (cbn [t_fired]; exact Hlt);
            destruct (IH t' x Hw Hf) as (H1 & H2 & H3 & H4)
        end.
        repeat split; [exact H1 | exact H2 | cbn [t_fired] in H3; lia | rewrite H4; reflexivity].
      + cbn [fst snd t_fired t_owed]. repeat split; [discriminate | lia | lia].
    - cbn [fst snd t_fired t_owed]. repeat split; [discriminate | lia | lia].
  Qed.

  Lemma receive_bounded :
    forall n t W, length (recs (t_st t)) < n -> t_fired t < cap ->
      snd (receive n t W) <> NoFuel /\ t_fired (fst (receive n t W)) <= cap.
  Proof.
    induction n as [| n IH]; intros t W Hn Hc; [lia |]. cbn [receive].
    destruct (fires W (recs (t_st t))) eqn:Ef; [| cbn [fst snd t_fired]; split; [discriminate | lia]].
    destruct (after_fire (S (t_fired t)) (negb (w_pers W))
                (w_peek W || existsb r_pers (sel W (recs (t_st t))))) eqn:Ea.
    - cbn [fst snd t_fired]. split; [discriminate | lia].
    - destruct (after_fire_cont _ _ _ Ea) as (_ & Hs & Hlt). apply orb_false_iff in Hs as [_ Hpers].
      assert (Hne : exists r, In r (sel W (recs (t_st t)))).
      { unfold fires in Ef. destruct (sel W (recs (t_st t))) as [| r rs]; [discriminate Ef |].
        exists r. left. reflexivity. }
      destruct Hne as [r Hr].
      assert (Hrp : r_pers r = false).
      { destruct (r_pers r) eqn:Er; [| reflexivity]. exfalso.
        assert (Hx : existsb r_pers (sel W (recs (t_st t))) = true)
          by (apply existsb_exists; exists r; split; [exact Hr | exact Er]).
        rewrite Hx in Hpers. discriminate Hpers. }
      assert (Hg : In r (filter (fun r0 => negb (r_pers r0)) (sel W (recs (t_st t)))))
        by (apply filter_In; split; [exact Hr | rewrite Hrp; reflexivity]).
      pose proof (drop_shrinks _ _ r Hg (sel_in _ _ _ Hr)) as Hd.
      match goal with
      | |- context [receive n ?t' W] =>
          assert (Hw : length (recs (t_st t')) < n) by (cbn [t_st recs]; lia);
          assert (Hf : t_fired t' < cap) by (cbn [t_fired]; exact Hlt);
          exact (IH t' W Hw Hf)
      end.
    - cbn [fst snd t_fired]. split; [discriminate | lia].
  Qed.

  Lemma tail_bounded :
    forall n t, 2 * (cap - t_fired t) + length (t_owed t) < n -> t_fired t <= cap ->
      snd (tail n t) <> NoFuel /\ t_fired (fst (tail n t)) <= cap.
  Proof.
    induction n as [| n IH]; intros t Hn Hc; [lia |]. cbn [tail].
    destruct (t_owed t) as [| x rest] eqn:Eo; cbn [length] in Hn.
    - destruct (t_rm t) eqn:Er; cbn [negb].
      + destruct (first_cascade (recs (t_st t)) (wtrs (t_st t))) as [W |] eqn:Ec.
        * destruct (cap <=? t_fired t) eqn:Ecap; [cbn [fst snd]; split; [discriminate | exact Hc] |].
          apply Nat.leb_gt in Ecap.
          destruct (sel W (recs (t_st t))) as [| lead others] eqn:Es; [cbn [fst snd]; split; [discriminate | exact Hc] |].
          destruct (after_fire _ _ _) eqn:Ea;
            match goal with
            | |- context [tail n ?t'] =>
                assert (Hm : 2 * (cap - t_fired t') + length (t_owed t') < n)
                  by (cbn [t_fired t_owed]; destruct (w_peek W && negb (r_pers lead)); cbn [length]; lia);
                assert (Hf : t_fired t' <= cap) by (cbn [t_fired]; lia);
                exact (IH t' Hm Hf)
            | |- _ => cbn [fst snd t_fired]; split; [discriminate | lia]
            end.
        * cbn [fst snd t_fired]. split; [discriminate | exact Hc].
      + cbn [fst snd]. split; [discriminate | exact Hc].
    - destruct (cap <=? t_fired t) eqn:Ecap; [cbn [fst snd]; split; [discriminate | exact Hc] |].
      apply Nat.leb_gt in Ecap.
      set (t0 := mkTx (t_st t) (t_fired t) rest (t_rm t) (t_spent t)).
      destruct (offer_bounded (offer_fuel t0) t0 x ltac:(unfold offer_fuel; lia) Ecap) as (H1 & H2 & H3 & H4).
      destruct (offer (offer_fuel t0) t0 x) as [t1 o] eqn:Eoff. cbn [fst snd] in H1, H2, H3, H4.
      destruct o.
      + apply IH; [| exact H2]. rewrite H4. unfold t0 in H3 |- *. cbn [t_owed t_fired] in H3 |- *. lia.
      + cbn [fst snd]. split; [discriminate | exact H2].
      + contradiction H1. reflexivity.
  Qed.

  (* Headline 4 (I-21, decision 49). A transaction fires at most cap times,
     and none of its loops runs out of the fuel that the plans compute: an
     offer removes the waiter of each fire that does not end it, a receive
     that fires again removes a record, and each step of the tail lowers
     2·(cap - fired) + |owed|. *)
  Theorem vdb_txn_bounded :
    0 < cap ->
    forall s W x,
      snd (run_receive s W) <> NoFuel /\ t_fired (fst (run_receive s W)) <= cap /\
      snd (run_offer s x) <> NoFuel /\ t_fired (fst (run_offer s x)) <= cap.
  Proof.
    intros Hcap s W x. unfold run_receive, run_offer.
    destruct (receive_bounded (receive_fuel (fresh s)) (fresh s) W ltac:(unfold receive_fuel; lia) Hcap)
      as [R1 R2].
    destruct (offer_bounded (offer_fuel (fresh s)) (fresh s) x ltac:(unfold offer_fuel; lia) Hcap)
      as (O1 & O2 & _ & _).
    destruct (receive (receive_fuel (fresh s)) (fresh s) W) as [t1 o1] eqn:E1.
    destruct (offer (offer_fuel (fresh s)) (fresh s) x) as [t2 o2] eqn:E2.
    cbn [fst snd] in R1, R2, O1, O2.
    assert (P1 : snd (match (t1, o1) with (t, Ended) => tail (tail_fuel t) t | r => r end) <> NoFuel /\
                 t_fired (fst (match (t1, o1) with (t, Ended) => tail (tail_fuel t) t | r => r end)) <= cap).
    { destruct o1; cbn [fst snd].
      - exact (tail_bounded (tail_fuel t1) t1 ltac:(unfold tail_fuel; lia) R2).
      - split; [discriminate | exact R2].
      - contradiction R1. reflexivity. }
    assert (P2 : snd (match (t2, o2) with (t, Ended) => tail (tail_fuel t) t | r => r end) <> NoFuel /\
                 t_fired (fst (match (t2, o2) with (t, Ended) => tail (tail_fuel t) t | r => r end)) <= cap).
    { destruct o2; cbn [fst snd].
      - exact (tail_bounded (tail_fuel t2) t2 ltac:(unfold tail_fuel; lia) O2).
      - split; [discriminate | exact O2].
      - contradiction O1. reflexivity. }
    destruct P1 as [A1 A2]. destruct P2 as [B1 B2].
    split; [exact A1 | split; [exact A2 | split; [exact B1 | exact B2]]].
  Qed.

  (* ── 9. Rule E: no record is lost ───────────────────────────────────── *)

  Definition ids (l : list rec) : list nat := map r_id l.

  (* Each id of R0 is stored, spent by a fire of a waiter that does not
     peek, or owed as a second send. *)
  Definition NL (R0 : list rec) (t : tx) : Prop :=
    forall r, In r R0 ->
      In (r_id r) (ids (recs (t_st t))) \/ In (r_id r) (t_spent t) \/ In (r_id r) (ids (t_owed t)).

  Lemma drop_or_id : forall out R i, In i (ids R) -> In i (ids (drop out R)) \/ In i (ids out).
  Proof.
    intros out R i H. unfold ids in H. apply in_map_iff in H as [r [<- Hr]]. unfold drop.
    destruct (has_id (r_id r) out) eqn:E.
    - right. unfold has_id in E. apply existsb_exists in E as [o [Ho Eo]]. apply Nat.eqb_eq in Eo.
      unfold ids. rewrite <- Eo. apply in_map. exact Ho.
    - left. unfold ids. apply in_map. apply filter_In. split; [exact Hr | rewrite E; reflexivity].
  Qed.

  Lemma offer_nl :
    forall n R0 t x,
      (forall r, In r R0 -> In (r_id r) (ids (recs (t_st t))) \/ In (r_id r) (t_spent t) \/
                            In (r_id r) (ids (t_owed t)) \/ r_id r = r_id x) ->
      snd (offer n t x) = Ended -> NL R0 (fst (offer n t x)).
  Proof.
    induction n as [| n IH]; intros R0 t x H He; [discriminate He |]. cbn [offer] in He |- *.
    destruct (first_offer (recs (t_st t)) x (wtrs (t_st t))) as [W |] eqn:E.
    - set (out := retired W (osel W (recs (t_st t)) x) x) in *.
      set (spent := negb (r_pers x) && negb (w_peek W)) in *.
      assert (H' : forall r, In r R0 ->
                In (r_id r) (ids (drop out (recs (t_st t)))) \/
                In (r_id r) (map r_id out ++ (if spent then [r_id x] else []) ++ t_spent t) \/
                In (r_id r) (ids (t_owed t)) \/ (spent = false /\ r_id r = r_id x)).
      { intros r Hr. destruct (H r Hr) as [Hs | [Hs | [Hs | Hs]]].
        - destruct (drop_or_id out _ _ Hs) as [Hd | Hd]; [left; exact Hd |].
          right. left. apply in_or_app. left. exact Hd.
        - right. left. apply in_or_app. right. apply in_or_app. right. exact Hs.
        - right. right. left. exact Hs.
        - destruct spent eqn:Esp.
          + right. left. apply in_or_app. right. apply in_or_app. left. rewrite Hs. left. reflexivity.
          + right. right. right. split; [reflexivity | exact Hs]. }
      destruct (after_fire (S (t_fired t)) spent (w_pers W)) eqn:Ea.
      + pose proof (after_fire_done _ _ _ Ea) as Hsp.
        intros r Hr. cbn [fst t_st recs t_spent t_owed].
        destruct (H' r Hr) as [A | [A | [A | [A _]]]];
          [left; exact A | right; left; exact A | right; right; exact A | rewrite Hsp in A; discriminate A].
      + apply IH; [| exact He].
        intros r Hr. cbn [t_st recs t_spent t_owed].
        destruct (H' r Hr) as [A | [A | [A | [_ A]]]];
          [left; exact A | right; left; exact A | right; right; left; exact A | right; right; right; exact A].
      + discriminate He.
    - intros r Hr. unfold ids. cbn [fst t_st recs t_spent t_owed map In].
      destruct (H r Hr) as [A | [A | [A | A]]].
      + left. right. exact A.
      + right. left. exact A.
      + right. right. exact A.
      + left. left. symmetry. exact A.
  Qed.

  Lemma receive_nl :
    forall n R0 t W, NL R0 t -> snd (receive n t W) = Ended -> NL R0 (fst (receive n t W)).
  Proof.
    induction n as [| n IH]; intros R0 t W H He; [discriminate He |]. cbn [receive] in He |- *.
    destruct (fires W (recs (t_st t))) eqn:Ef; [| exact H].
    set (gone := filter (fun r => negb (r_pers r)) (sel W (recs (t_st t)))) in *.
    assert (H' : NL R0 (mkTx (mkSt (drop gone (recs (t_st t))) (wtrs (t_st t))) (S (t_fired t))
                        (if w_peek W then gone ++ t_owed t else t_owed t) true
                        (if w_peek W then t_spent t else map r_id gone ++ t_spent t))).
    { intros r Hr. cbn [t_st recs t_spent t_owed]. destruct (H r Hr) as [A | [A | A]].
      - destruct (drop_or_id gone _ _ A) as [D | D]; [left; exact D |].
        destruct (w_peek W).
        + right. right. unfold ids. rewrite map_app. apply in_or_app. left. exact D.
        + right. left. apply in_or_app. left. exact D.
      - right. left. destruct (w_peek W); [exact A | apply in_or_app; right; exact A].
      - right. right. destruct (w_peek W); [unfold ids; rewrite map_app; apply in_or_app; right; exact A | exact A]. }
    destruct (after_fire _ _ _); [exact H' | apply IH; [exact H' | exact He] | discriminate He].
  Qed.

  Lemma tail_nl :
    forall n R0 t, NL R0 t -> snd (tail n t) = Ended -> NL R0 (fst (tail n t)) /\ t_owed (fst (tail n t)) = [].
  Proof.
    induction n as [| n IH]; intros R0 t H He; [discriminate He |]. cbn [tail] in He |- *.
    destruct (t_owed t) as [| x rest] eqn:Eo.
    - destruct (t_rm t) eqn:Er; cbn [negb] in He |- *.
      + destruct (first_cascade (recs (t_st t)) (wtrs (t_st t))) as [W |] eqn:Ec.
        * destruct (cap <=? t_fired t); [discriminate He |].
          destruct (sel W (recs (t_st t))) as [| lead others] eqn:Es; [discriminate He |].
          set (ret := if w_peek W then [] else filter (fun r => negb (r_pers r)) others) in *.
          set (out := (if negb (r_pers lead) then [lead] else []) ++ ret) in *.
          assert (H' : NL R0 (mkTx (mkSt (drop out (recs (t_st t))) (after_w W (wtrs (t_st t)))) (S (t_fired t))
                                   (if w_peek W && negb (r_pers lead) then [lead] else []) true
                                   (map r_id ret ++ (if negb (r_pers lead) && negb (w_peek W) then [r_id lead] else [])
                                    ++ t_spent t))).
          { intros r Hr. cbn [t_st recs t_spent t_owed]. destruct (H r Hr) as [A | [A | A]].
            - destruct (drop_or_id out _ _ A) as [D | D]; [left; exact D |].
              unfold out, ids in D. rewrite map_app in D. apply in_app_or in D as [D | D].
              + destruct (negb (r_pers lead)) eqn:El; [| destruct D].
                cbn [map In] in D. destruct D as [D | []].
                destruct (w_peek W) eqn:Ep; cbn [andb].
                * right. right. unfold ids. cbn [map In]. left. exact D.
                * right. left. apply in_or_app. right. apply in_or_app. left. left. exact D.
              + right. left. apply in_or_app. left. exact D.
            - right. left. apply in_or_app. right. apply in_or_app. right. exact A.
            - rewrite Eo in A. destruct A. }
          destruct (after_fire _ _ _); [exact (IH R0 _ H' He) | exact (IH R0 _ H' He) | discriminate He].
        * cbn [fst]. split; [| reflexivity]. intros r Hr. cbn [t_st recs t_spent t_owed].
          destruct (H r Hr) as [A | [A | A]]; [left; exact A | right; left; exact A | rewrite Eo in A; destruct A].
      + cbn [fst]. split; [exact H | exact Eo].
    - destruct (cap <=? t_fired t); [discriminate He |].
      set (t0 := mkTx (t_st t) (t_fired t) rest (t_rm t) (t_spent t)) in *.
      destruct (offer (offer_fuel t0) t0 x) as [t1 o] eqn:Eoff.
      destruct o; [| discriminate He | discriminate He].
      assert (H1 : NL R0 t1).
      { replace t1 with (fst (offer (offer_fuel t0) t0 x)) by (rewrite Eoff; reflexivity).
        apply offer_nl; [| rewrite Eoff; reflexivity].
        intros r Hr. destruct (H r Hr) as [A | [A | A]].
        - left. exact A.
        - right. left. exact A.
        - rewrite Eo in A. unfold ids in A. cbn [map In] in A.
          destruct A as [A | A]; [right; right; right; symmetry; exact A | right; right; left; exact A]. }
      exact (IH R0 t1 H1 He).
  Qed.

  (* Headline 5 (Rule E, decision 45). No record is lost: a record of the
     pre-state is in the post-state of a transaction that ends, by its id,
     or a fire of a waiter that does not peek spent it. A peek fire removes
     its non-persistent members and owes them as second sends, and the tail
     offers each one until it is stored again or a fire spends it. *)
  Theorem vdb_no_record_lost :
    forall s W x,
      (forall t, run_receive s W = (t, Ended) ->
         forall r, In r (recs s) -> In (r_id r) (ids (recs (t_st t))) \/ In (r_id r) (t_spent t)) /\
      (forall t, run_offer s x = (t, Ended) ->
         forall r, In r (recs s) -> In (r_id r) (ids (recs (t_st t))) \/ In (r_id r) (t_spent t)).
  Proof.
    intros s W x.
    assert (H0 : NL (recs s) (fresh s)) by (intros r0 Hr0; left; unfold ids; apply in_map; exact Hr0).
    split.
    - intros t Ht r Hr. unfold run_receive in Ht.
      destruct (receive (receive_fuel (fresh s)) (fresh s) W) as [t1 o] eqn:E.
      destruct o; [| discriminate Ht | discriminate Ht].
      assert (H1 : NL (recs s) t1).
      { replace t1 with (fst (receive (receive_fuel (fresh s)) (fresh s) W)) by (rewrite E; reflexivity).
        apply receive_nl; [exact H0 | rewrite E; reflexivity]. }
      destruct (tail_nl _ (recs s) t1 H1 ltac:(rewrite Ht; reflexivity)) as [H2 H3].
      rewrite Ht in H2, H3. cbn [fst] in H2, H3.
      destruct (H2 r Hr) as [A | [A | A]]; [left; exact A | right; exact A | rewrite H3 in A; destruct A].
    - intros t Ht r Hr. unfold run_offer in Ht.
      destruct (offer (offer_fuel (fresh s)) (fresh s) x) as [t1 o] eqn:E.
      destruct o; [| discriminate Ht | discriminate Ht].
      assert (H1 : NL (recs s) t1).
      { replace t1 with (fst (offer (offer_fuel (fresh s)) (fresh s) x)) by (rewrite E; reflexivity).
        apply offer_nl; [| rewrite E; reflexivity].
        intros r0 Hr0. destruct (H0 r0 Hr0) as [A | [A | A]];
          [left; exact A | right; left; exact A | right; right; left; exact A]. }
      destruct (tail_nl _ (recs s) t1 H1 ltac:(rewrite Ht; reflexivity)) as [H2 H3].
      rewrite Ht in H2, H3. cbn [fst] in H2, H3.
      destruct (H2 r Hr) as [A | [A | A]]; [left; exact A | right; exact A | rewrite H3 in A; destruct A].
  Qed.
End VectorDb.

Arguments mkRec {V} r_id r_val r_pers.
Arguments r_id {V} r.
Arguments r_val {V} r.
Arguments r_pers {V} r.


(* ── 10. An instance: the hypotheses hold, and the cascade is needed ──── *)

(* The lexicographic order of keys, as in the instance of JoinCascade.v. *)
Fixpoint lex (a b : list nat) : bool :=
  match a, b with
  | [], _ => true
  | _ :: _, [] => false
  | x :: a', y :: b' => (x <? y) || ((x =? y) && lex a' b')
  end.

Lemma lex_total : forall a b, length a = length b -> lex a b = true \/ lex b a = true.
Proof.
  induction a as [| x a IH]; intros [| y b] Hl; cbn [lex length] in Hl |- *; try discriminate; [left; reflexivity |].
  destruct (Nat.lt_trichotomy x y) as [Hxy | [-> | Hxy]].
  - left. apply orb_true_iff. left. apply Nat.ltb_lt. exact Hxy.
  - rewrite Nat.ltb_irrefl, Nat.eqb_refl. cbn [orb andb]. apply IH. lia.
  - right. apply orb_true_iff. left. apply Nat.ltb_lt. exact Hxy.
Qed.

Lemma lex_antisym : forall a b, lex a b = true -> lex b a = true -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H1 H2; cbn [lex] in H1, H2; try discriminate; [reflexivity |].
  apply orb_true_iff in H1, H2. destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - apply Nat.ltb_lt in H1, H2. lia.
  - apply Nat.ltb_lt in H1. apply andb_true_iff in H2 as [H2 _]. apply Nat.eqb_eq in H2. lia.
  - apply Nat.ltb_lt in H2. apply andb_true_iff in H1 as [H1 _]. apply Nat.eqb_eq in H1. lia.
  - apply andb_true_iff in H1 as [E1 H1]. apply andb_true_iff in H2 as [_ H2]. apply Nat.eqb_eq in E1. subst y.
    f_equal. exact (IH b H1 H2).
Qed.

Lemma lex_trans : forall a b c, lex a b = true -> lex b c = true -> lex a c = true.
Proof.
  induction a as [| x a IH]; intros [| y b] [| z c] H1 H2; cbn [lex] in H1, H2 |- *; try discriminate; try reflexivity.
  apply orb_true_iff in H1, H2. apply orb_true_iff.
  destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - left. apply Nat.ltb_lt in H1, H2. apply Nat.ltb_lt. lia.
  - left. apply Nat.ltb_lt in H1. apply andb_true_iff in H2 as [H2 _]. apply Nat.eqb_eq in H2. apply Nat.ltb_lt. lia.
  - left. apply Nat.ltb_lt in H2. apply andb_true_iff in H1 as [H1 _]. apply Nat.eqb_eq in H1. apply Nat.ltb_lt. lia.
  - right. apply andb_true_iff in H1 as [E1 H1]. apply andb_true_iff in H2 as [E2 H2]. apply andb_true_iff. split.
    + apply Nat.eqb_eq in E1, E2. apply Nat.eqb_eq. lia.
    + exact (IH b c H1 H2).
Qed.

(* Insertion sort by a boolean order. *)
Section Sort.
  Variable X : Type.
  Variable le : X -> X -> bool.

  Fixpoint ins (x : X) (l : list X) : list X :=
    match l with [] => [x] | y :: r => if le x y then x :: y :: r else y :: ins x r end.

  Definition isort (l : list X) : list X := fold_right ins [] l.

  Lemma ins_perm : forall x l, Permutation (ins x l) (x :: l).
  Proof.
    intros x l. induction l as [| y r IH]; simpl; [apply Permutation_refl |].
    destruct (le x y); [apply Permutation_refl |].
    apply Permutation_trans with (y :: x :: r); [apply perm_skip; exact IH | apply perm_swap].
  Qed.

  Lemma isort_perm : forall l, Permutation (isort l) l.
  Proof.
    induction l as [| x r IH]; simpl; [apply Permutation_refl |].
    apply Permutation_trans with (x :: isort r); [apply ins_perm | apply perm_skip; exact IH].
  Qed.

  Lemma ins_split : forall e L, exists l1 l2, L = l1 ++ l2 /\ ins e L = l1 ++ e :: l2.
  Proof.
    intros e L. induction L as [| z L IH]; [exists [], []; split; reflexivity |]. cbn [ins].
    destruct (le e z); [exists [], (z :: L); split; reflexivity |].
    destruct IH as (l1 & l2 & E1 & E2). exists (z :: l1), l2. rewrite E2, E1. split; reflexivity.
  Qed.
End Sort.

Arguments ins {X} le x l.
Arguments isort {X} le l.

Lemma perm_filter :
  forall (A : Type) (f : A -> bool) l l', Permutation l l' -> Permutation (filter f l) (filter f l').
Proof.
  intros A f l l' H. induction H as [| x l1 l2 _ IH | x y l | l1 l2 l3 _ IH1 _ IH2]; cbn [filter].
  - constructor.
  - destruct (f x); [apply perm_skip |]; exact IH.
  - destruct (f x), (f y); try apply perm_swap; apply Permutation_refl.
  - exact (Permutation_trans IH1 IH2).
Qed.

(* The order of the instance: all the fields of a record, the id first. *)
Definition wkey (r : rec nat) : list nat := [r_id r; r_val r; if r_pers r then 1 else 0].

Definition wle (a b : rec nat) : bool := lex (wkey a) (wkey b).

Lemma wle_total : forall a b, wle a b = true \/ wle b a = true.
Proof. intros a b. apply lex_total. reflexivity. Qed.

Lemma wle_antisym : forall a b, wle a b = true -> wle b a = true -> a = b.
Proof.
  intros [i1 v1 p1] [i2 v2 p2] H1 H2. pose proof (lex_antisym _ _ H1 H2) as E.
  unfold wkey in E. cbn in E. injection E as Ei Ev Ep. subst.
  destruct p1, p2; try discriminate; reflexivity.
Qed.

Lemma wle_trans : forall a b c, wle a b = true -> wle b c = true -> wle a c = true.
Proof. intros a b c. apply lex_trans. Qed.

Lemma wins_comm : forall x y L, ins wle x (ins wle y L) = ins wle y (ins wle x L).
Proof.
  intros x y L. induction L as [| z L IH].
  - cbn [ins]. destruct (wle x y) eqn:Exy, (wle y x) eqn:Eyx; cbn [ins]; rewrite ?Exy, ?Eyx; try reflexivity.
    + rewrite (wle_antisym x y Exy Eyx). reflexivity.
    + destruct (wle_total x y) as [E | E]; congruence.
  - cbn [ins]. destruct (wle y z) eqn:Eyz, (wle x z) eqn:Exz; cbn [ins]; rewrite ?Eyz, ?Exz.
    + destruct (wle x y) eqn:Exy, (wle y x) eqn:Eyx; try reflexivity.
      * rewrite (wle_antisym x y Exy Eyx). reflexivity.
      * destruct (wle_total x y) as [E | E]; congruence.
    + destruct (wle x y) eqn:Exy; [rewrite (wle_trans x y z Exy Eyz) in Exz; discriminate |]. reflexivity.
    + destruct (wle y x) eqn:Eyx; [rewrite (wle_trans y x z Eyx Exz) in Eyz; discriminate |]. reflexivity.
    + rewrite IH. reflexivity.
Qed.

Lemma wisort_set : forall l l', Permutation l l' -> isort wle l = isort wle l'.
Proof.
  intros l l' H. induction H as [| x l1 l2 _ IH | x y l | l1 l2 l3 _ IH1 _ IH2].
  - reflexivity.
  - cbn [isort fold_right]. fold (isort wle l1). fold (isort wle l2). rewrite IH. reflexivity.
  - cbn [isort fold_right]. apply wins_comm.
  - rewrite IH1. exact IH2.
Qed.

(* The parameters of the instance: a query q admits the records whose
   value is at least q, in the order of the ids; a single-mode pattern is a
   list of the values that it binds, and a list-mode pattern is the exact
   list of values. *)
Definition welig (q : nat) (r : rec nat) : bool := q <=? r_val r.
Definition wesort (q : nat) (l : list (rec nat)) : list (rec nat) := isort wle (filter (welig q) l).

Definition wbinds1 (p : list nat) (v : nat) : bool := existsb (Nat.eqb v) p.

Fixpoint nlist_eqb (a b : list nat) : bool :=
  match a, b with
  | [], [] => true
  | x :: a', y :: b' => (x =? y) && nlist_eqb a' b'
  | _, _ => false
  end.

Definition wbindsL (p : list nat) (vs : list nat) : bool := nlist_eqb vs p.
Definition wadmits (W : wtr nat (list nat)) (vs : list nat) : bool := true.

(* The hypotheses of the section hold for the instance. *)
Corollary vdb_parameters_valid :
  (forall q l, Permutation (wesort q l) (filter (welig q) l)) /\
  (forall q l l', Permutation l l' -> wesort q l = wesort q l') /\
  (forall q x l, exists l1 l2, wesort q l = l1 ++ l2 /\
     wesort q (x :: l) = (if welig q x then l1 ++ x :: l2 else l1 ++ l2)) /\
  (forall W vs, w_free nat (list nat) W = true -> wadmits W vs = true).
Proof.
  split; [intros q l; apply isort_perm |]. split.
  - intros q l l' H. unfold wesort. apply wisort_set. apply perm_filter. exact H.
  - split; [| intros W vs _; reflexivity].
    intros q x l. unfold wesort. cbn [filter]. destruct (welig q x).
    + destruct (ins_split _ wle x (isort wle (filter (welig q) l))) as (l1 & l2 & E1 & E2).
      exists l1, l2. split; [exact E1 |].
      change (isort wle (x :: filter (welig q) l)) with (ins wle x (isort wle (filter (welig q) l))).
      exact E2.
    + exists (isort wle (filter (welig q) l)), []. rewrite app_nil_r. split; reflexivity.
Qed.

(* The state of CE-2: img (value 0) and two documents (value 1), and a
   parked list waiter for two records whose pattern binds only the list of
   two documents. A single-mode receive takes img. *)
Definition w_img : rec nat := mkRec 0 0 false.
Definition w_doc1 : rec nat := mkRec 1 1 false.
Definition w_doc2 : rec nat := mkRec 2 1 false.
Definition w_list : wtr nat (list nat) := mkWtr nat (list nat) 10 0 [1; 1] (Listed 2) true false false.
Definition w_take : wtr nat (list nat) := mkWtr nat (list nat) 11 0 [0] Single true false false.
Definition w_state : vst nat nat (list nat) := mkSt nat nat (list nat) [w_img; w_doc1; w_doc2] [w_list].

Definition WINV := INV nat nat (list nat) wesort wbinds1 wbindsL wadmits.
Definition Wfires := fires nat nat (list nat) wesort wbinds1 wbindsL wadmits.
Definition Wfresh := fresh nat nat (list nat) w_state.
Definition Wreceive := receive nat nat (list nat) 64 wesort wbinds1 wbindsL wadmits
                         (receive_fuel nat nat (list nat) Wfresh) Wfresh w_take.
Definition Wrun := run_receive nat nat (list nat) 64 welig wesort wbinds1 wbindsL wadmits w_state w_take.

(* Headline 3 (LM-b, decision 66, the counterexample CE-2 of the
   arbitration). The state holds INV. A single-mode receive takes img, and
   without LIST-CASCADE its transaction ends with the parked list waiter
   fireable: its first two records are now the two documents, which its
   pattern binds. With the tail, the cascade fires the list waiter, and the
   transaction ends in INV. So LM-b needs the cascade. *)
Theorem vdb_list_needs_cascade :
  WINV w_state /\
  snd Wreceive = Ended /\ Wfires w_list (recs nat nat (list nat) (t_st nat nat (list nat) (fst Wreceive))) = true /\
  snd Wrun = Ended /\ WINV (t_st nat nat (list nat) (fst Wrun)).
Proof.
  split.
  { intros W HW _. destruct HW as [<- | []]. vm_compute. reflexivity. }
  split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |].
  intros W HW. vm_compute in HW. destruct HW.
Qed.

(* ── Headline assumptions ────────────────────────────────────────────── *)
Print Assumptions vdb_join_removal_exposes_nothing.
Print Assumptions vdb_no_pending_match.
Print Assumptions vdb_list_needs_cascade.
Print Assumptions vdb_txn_bounded.
Print Assumptions vdb_no_record_lost.
