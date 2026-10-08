(* ═══════════════════════════════════════════════════════════════════════════
   DataCollection.v — the data side of the bag discipline under the scan driver
   (P5a: decisions 37, 41, 42, 43 and 45)

   A bag collection is a multiset of records on its data channel D(sid,k). Its
   plans are the scan driver of P5 (spaces/disciplines/scan.rs): a receive
   visits the records in the canonical order of DR-75 (decision 41) and takes
   the first record that its pattern binds, and it checks its guard once, on
   that record (decision 37). A fire removes the record that it binds unless
   the record is persistent. A peek fire also removes a non-persistent
   record, as the host COMM does, and the operation then sends the record
   again: the record is offered to the waiters first, so a guarded waiter that
   admits it fires, and it is stored when no waiter takes it (decision 45,
   Rule E). That second send has the identity of the record and registers the
   cost authority of the record. Every read and copy of the driver is reserved
   before it is made (decision 42).

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md, A.7 (OFFER-SCAN,
     RECEIVE-SCAN, RESTORE-SCAN, the defined effects of the bag), A.12 (the
     effects of decisions 42, 43 and 45), A.13 (this module: F1 to F5 and
     scan_trace_covered). The P5 design v2, 2.8.1 (F1 to F5).
   - Plan: decisions 37 (the current guard rule), 41 (source-hash order), 42
     (reserve before each read) and 45 (Rule E).
   - Code: rholang/src/rust/interpreter/spaces/disciplines/scan.rs and
     bag.rs of the BAG batch of P5 wave W2. The line numbers below are the
     line numbers of that batch.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   visit, canon, insert_key space_store_handle.rs canonical_data_order and
                            candidate_order.rs canonical_order (DR-75): the
                            records by the hash of their source produce,
                            ties by digest, then by index. Equal source
                            hashes are byte-identical records, so a sort of
                            the keys is the canonical order.
   first_match,
   receive_choice           scan.rs:228-314 (receive: first_bound_record at
                            533-553, then Cx::admits once, then park)
   op, on_records           the staged operations of the driver and the
                            effect of each one on the records of D:
                            Store (Cx::store_on), Park (Cx::park), FireIn
                            (Cx::fire_offer_on: the incoming datum is never
                            stored) and FireSt (Cx::fire_receive_group: the
                            bound record leaves unless it is persistent,
                            space_store_handle.rs removed_records)
   fits, offer_pass, offer  scan.rs:401-492 (offer_scan with
                            first_admitting_waiter at 497-528: one pass
                            over the waiters in canonical order, a guard
                            refusal moves on, after_fire of FireRounds)
   host_send                the host program of one send: the produce of
                            the host RSpace scans every waiter in canonical
                            order and fires the first one that fits
                            (ops_produce.rs, space_matcher.rs), and the
                            reducer sends again after a fire of a
                            persistent datum or of a peek waiter
                            (reduce.rs continue_produce_process,
                            produce_peeks)
   peek_plan                scan.rs:228-314, 701-711 and 722-754 (receive
                            with restore_in and restore_scan: the second
                            send of the removed record with
                            Introduction::for_record), and 723-744
                            (restored_send: the re-issue at the cap)
   host_peek                the host program of a peek receive: the COMM,
                            then produce_peeks with the authority of the
                            removed record (reduce.rs produce_peeks)
   scan_trace               the reservations of the driver: the canonical
                            order (canonical_data_order), one payload decode
                            for each tested record (unwrap_datum_metered),
                            one metered match for each tested record, the
                            guard, the copy of the selection, the copies of
                            the staged operation, and the prepaid outcome
                            (prepay_continuation_unwrap)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A record is its source key: the hash of its source produce. The hash
     covers the channel, the record bytes and the persist flag, so equal keys
     are byte-identical records (C.3 of the decision 45 design). The payload,
     the persist flag and the cost authority of a record are functions of its
     key (val, persistent, rauth). A list of keys is the multiset of records.
   - A waiter of the offer is its key, the hash of its source consume. Its
     pattern, guard, persist flag and peek flag are functions of its key. A
     waiter list holds no duplicate (ContinuationCollection.v,
     park_byte_identity_idempotent).
   - spatial is the metered match of a pattern against a payload
     (SpaceMatcher::bind) and admits is the guard (SpaceMatcher::admits). The
     model takes both as section variables: every theorem holds for every
     matcher.
   - A bag has no counter, so every ord is 0 and the record of an element is
     the same in every round (decision 43). The model needs no ord.
   - rounds counts the fires that FIRE_ROUND_CAP still allows after the next
     fire (I-21). A divergent pair or the cap ends a plan with Reissue.
   - The host program sends again after each fire that leaves its datum in
     flight. It loops forever on a divergent pair, where the agent re-issues.
     So the equivalence of the agent and the host holds for plans that settle.
   - The metering model restates the rule of AgentReadCoverage.v (DR-88 of
     feature/cost-accounted-rho): a self-metered step reserves its units and
     then reads them, and a trace is covered when every prefix reads at most
     what it reserved. The model counts one host-work dimension. Each
     dimension satisfies the rule on its own.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. bag_visit_order_canonical         F1: the order is sorted, a
                                         permutation of the records, and the
                                         same for every list order
    2. bag_consume_first_spatial_match   F2: a receive fires the first
                                         record in order that its pattern
                                         binds when its guard admits it, and
                                         it parks on a refusal (decision 37)
    3. bag_persistent_match_not_removed  F5: a fire on a persistent record
                                         leaves it
    4. bag_multiplicity_preserved        F3: a store adds exactly one record,
                                         and a fire removes exactly its bound
                                         non-persistent record
    5. bag_removal_keeps_equal_duplicate F4: the removal of one of two equal
                                         records leaves the other
    6. bag_peek_restore_is_host_second_send
                                         Rule E: a peek receive stages the
                                         host program, the second send of the
                                         removed record with its own
                                         authority included, and a record
                                         that no waiter takes is stored again
    7. scan_trace_covered                decision 42: the trace of a scan
                                         reads after it reserves

   The negative controls of scripts/verify-reified/rocq-negative-controls.py
   (fragment of the BAG batch) mutate one rule each: the order by list
   position, the guard checked on every candidate, the removal of a
   persistent record, the removal of every equal record, a store that adds
   nothing, a peek without restore, a restore with the authority of the
   operation, a restore without re-offer, and a decode that reads before it
   reserves.

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

(* ── 1. The canonical order (DR-75, decision 41) ─────────────────────────── *)

(* The insertion of the key [k] into a sorted list. *)
Fixpoint insert_key (k : nat) (l : list nat) : list nat :=
  match l with
  | [] => [k]
  | h :: t => if k <=? h then k :: h :: t else h :: insert_key k t
  end.

(* The canonical order of the entries [l]: their keys in ascending order. *)
Fixpoint canon (l : list nat) : list nat :=
  match l with
  | [] => []
  | h :: t => insert_key h (canon t)
  end.

Lemma insert_key_perm : forall k l, Permutation (k :: l) (insert_key k l).
Proof.
  intros k l. induction l as [| h t IH]; cbn; [reflexivity |].
  destruct (k <=? h); [reflexivity |].
  eapply perm_trans; [apply perm_swap |]. apply perm_skip. exact IH.
Qed.

Lemma canon_perm : forall l, Permutation l (canon l).
Proof.
  induction l as [| h t IH]; cbn; [reflexivity |].
  eapply perm_trans; [apply perm_skip; exact IH | apply insert_key_perm].
Qed.

Lemma insert_key_hdrel : forall k h l, h <= k -> HdRel le h l -> HdRel le h (insert_key k l).
Proof.
  intros k h l Hhk Hrel. destruct l as [| x t]; cbn.
  - constructor. exact Hhk.
  - destruct (k <=? x); constructor; [exact Hhk | inversion Hrel; assumption].
Qed.

Lemma insert_key_sorted : forall k l, Sorted le l -> Sorted le (insert_key k l).
Proof.
  intros k l. induction l as [| h t IH]; intros Hs; cbn.
  - repeat constructor.
  - destruct (k <=? h) eqn:E.
    + constructor; [exact Hs | constructor; apply Nat.leb_le; exact E].
    + apply Nat.leb_gt in E. inversion Hs as [| ? ? Ht Hrel]; subst.
      constructor; [apply IH; exact Ht | apply insert_key_hdrel; [lia | exact Hrel]].
Qed.

Lemma canon_sorted : forall l, Sorted le (canon l).
Proof.
  induction l as [| h t IH]; cbn; [constructor | apply insert_key_sorted; exact IH].
Qed.

(* The head of a sorted list is below every later entry. *)
Lemma sorted_le_all : forall a l, Sorted le (a :: l) -> forall x, In x l -> a <= x.
Proof.
  intros a l. revert a. induction l as [| b t IH]; intros a Hs x Hin; [destruct Hin |].
  inversion Hs as [| ? ? Ht Hrel]; subst. inversion Hrel; subst.
  destruct Hin as [-> | Hin]; [assumption |].
  specialize (IH b Ht x Hin). lia.
Qed.

(* Two sorted lists of keys that are permutations of each other are equal. *)
Lemma sorted_perm_unique : forall l l',
  Sorted le l -> Sorted le l' -> Permutation l l' -> l = l'.
Proof.
  induction l as [| a t IH]; intros l' Hs Hs' Hp.
  - symmetry. apply Permutation_nil. exact Hp.
  - destruct l' as [| b t'].
    + apply Permutation_sym, Permutation_nil in Hp. discriminate.
    + assert (Hab : a = b).
      { assert (Ha : In a (b :: t')) by (eapply Permutation_in; [exact Hp | left; reflexivity]).
        assert (Hb : In b (a :: t)).
        { eapply Permutation_in; [symmetry; exact Hp | left; reflexivity]. }
        destruct Ha as [Ha | Ha]; [symmetry; exact Ha |].
        destruct Hb as [Hb | Hb]; [exact Hb |].
        pose proof (sorted_le_all b t' Hs' a Ha).
        pose proof (sorted_le_all a t Hs b Hb). lia. }
      subst b. f_equal.
      apply IH.
      * inversion Hs; assumption.
      * inversion Hs'; assumption.
      * eapply Permutation_cons_inv. exact Hp.
Qed.

Lemma canon_permutation_invariant : forall l l', Permutation l l' -> canon l = canon l'.
Proof.
  intros l l' Hp. apply sorted_perm_unique; try apply canon_sorted.
  eapply perm_trans; [symmetry; apply canon_perm |].
  eapply perm_trans; [exact Hp | apply canon_perm].
Qed.

(* The order in which the driver visits the records of a collection: the
   canonical order (Cx::canonical_data_order). *)
Definition visit (l : list nat) : list nat := canon l.

(* F1 bag_visit_order_canonical: the visit order of the records is sorted by
   source key, holds the records, and does not depend on the list order of
   the store. *)
Theorem bag_visit_order_canonical : forall l l',
  Sorted le (visit l) /\ Permutation l (visit l) /\
  (Permutation l l' -> visit l = visit l').
Proof.
  intros l l'. unfold visit. split; [apply canon_sorted |].
  split; [apply canon_perm | apply canon_permutation_invariant].
Qed.

(* ── 2. Lists of keys ────────────────────────────────────────────────────── *)

(* The number of occurrences of the key [j] in [l]. *)
Fixpoint count (j : nat) (l : list nat) : nat :=
  match l with
  | [] => 0
  | h :: t => (if h =? j then 1 else 0) + count j t
  end.

(* The removal of the first occurrence of [k]: a removal by record identity
   at its place in the staged view (space_store_handle.rs claim_positions). *)
Fixpoint remove_one (k : nat) (l : list nat) : list nat :=
  match l with
  | [] => []
  | h :: t => if h =? k then t else h :: remove_one k t
  end.

Lemma count_remove_one : forall k l j,
  count j (remove_one k l) + (if (k =? j) && (0 <? count j l) then 1 else 0) = count j l.
Proof.
  intros k l j. induction l as [| h t IH]; cbn [remove_one count].
  - destruct (k =? j); reflexivity.
  - destruct (Nat.eqb_spec h k) as [-> | Hhk].
    + destruct (Nat.eqb_spec k j) as [-> | Hkj]; cbn; lia.
    + cbn [count].
      assert (Hind : (k =? j) && (0 <? (if h =? j then 1 else 0) + count j t)
                     = (k =? j) && (0 <? count j t)).
      { destruct (Nat.eqb_spec k j) as [-> | Hkj]; [| reflexivity].
        destruct (Nat.eqb_spec h j) as [Hhj | Hhj]; [contradiction | reflexivity]. }
      rewrite Hind. lia.
Qed.

(* Of a record that occurs, the removal takes exactly one occurrence. *)
Lemma count_remove_one_in : forall k rs, 1 <= count k rs -> count k (remove_one k rs) + 1 = count k rs.
Proof.
  intros k rs H. pose proof (count_remove_one k rs k) as Hc.
  rewrite Nat.eqb_refl in Hc. cbn [andb] in Hc.
  destruct (Nat.ltb_spec 0 (count k rs)) as [_ | Hlt]; [exact Hc | lia].
Qed.

Lemma count_in : forall j l, In j l -> 1 <= count j l.
Proof.
  intros j l. induction l as [| h t IH]; cbn; intros Hin; [destruct Hin |].
  destruct Hin as [-> | Hin]; [rewrite Nat.eqb_refl; lia |].
  specialize (IH Hin). lia.
Qed.

Lemma find_some_split : forall (f : nat -> bool) l x,
  find f l = Some x <->
  exists pre post, l = pre ++ x :: post /\ (forall j, In j pre -> f j = false) /\ f x = true.
Proof.
  intros f l x. split.
  - induction l as [| h t IH]; cbn; intros H; [discriminate |].
    destruct (f h) eqn:E.
    + injection H as <-. exists [], t.
      refine (conj eq_refl (conj _ E)). intros j [].
    + destruct (IH H) as (pre & post & -> & Hpre & Hx).
      exists (h :: pre), post. refine (conj eq_refl (conj _ Hx)).
      intros j [<- | Hj]; [exact E | exact (Hpre j Hj)].
  - intros (pre & post & -> & Hpre & Hx). induction pre as [| h t IH]; cbn.
    + rewrite Hx. reflexivity.
    + rewrite (Hpre h (or_introl eq_refl)). apply IH.
      intros j Hj. apply Hpre. right. exact Hj.
Qed.

Lemma find_none_all : forall (f : nat -> bool) l,
  (forall j, In j l -> f j = false) -> find f l = None.
Proof.
  intros f l. induction l as [| h t IH]; cbn; intros H; [reflexivity |].
  rewrite (H h (or_introl eq_refl)). apply IH. intros j Hj. apply H. right. exact Hj.
Qed.

Lemma find_app_skip : forall (f : nat -> bool) pre post,
  (forall j, In j pre -> f j = false) -> find f (pre ++ post) = find f post.
Proof.
  intros f pre post. induction pre as [| h t IH]; cbn; intros H; [reflexivity |].
  rewrite (H h (or_introl eq_refl)). apply IH. intros j Hj. apply H. right. exact Hj.
Qed.

Lemma remove_one_not_in : forall k l, ~ In k l -> remove_one k l = l.
Proof.
  intros k l. induction l as [| h t IH]; cbn; intros Hn; [reflexivity |].
  destruct (Nat.eqb_spec h k) as [-> | Hhk]; [exfalso; apply Hn; left; reflexivity |].
  f_equal. apply IH. intros Hin. apply Hn. right. exact Hin.
Qed.

Lemma remove_one_app : forall k pre post, ~ In k pre -> remove_one k (pre ++ k :: post) = pre ++ post.
Proof.
  intros k pre post. induction pre as [| h t IH]; cbn; intros Hn.
  - rewrite Nat.eqb_refl. reflexivity.
  - destruct (Nat.eqb_spec h k) as [-> | Hhk]; [exfalso; apply Hn; left; reflexivity |].
    f_equal. apply IH. intros Hin. apply Hn. right. exact Hin.
Qed.

Lemma remove_one_cons_perm : forall k l, In k l -> Permutation l (k :: remove_one k l).
Proof.
  intros k l. induction l as [| h t IH]; cbn; intros Hin; [destruct Hin |].
  destruct (Nat.eqb_spec h k) as [-> | Hhk]; [reflexivity |].
  destruct Hin as [Hin | Hin]; [contradiction |].
  eapply perm_trans; [apply perm_skip; exact (IH Hin) | apply perm_swap].
Qed.

Lemma remove_one_perm : forall k l l',
  Permutation l l' -> Permutation (remove_one k l) (remove_one k l').
Proof.
  intros k l l' Hp. destruct (in_dec Nat.eq_dec k l) as [Hin | Hn].
  - assert (Hin' : In k l') by (eapply Permutation_in; [exact Hp | exact Hin]).
    apply (Permutation_cons_inv (a := k)).
    eapply perm_trans; [symmetry; apply remove_one_cons_perm; exact Hin |].
    eapply perm_trans; [exact Hp | apply remove_one_cons_perm; exact Hin'].
  - assert (Hn' : ~ In k l').
    { intros Hin'. apply Hn. eapply Permutation_in; [symmetry; exact Hp | exact Hin']. }
    rewrite (remove_one_not_in k l Hn), (remove_one_not_in k l' Hn'). exact Hp.
Qed.

Lemma remove_one_incl : forall k l x, In x (remove_one k l) -> In x l.
Proof.
  intros k l x. induction l as [| h t IH]; cbn; intros Hin; [exact Hin |].
  destruct (h =? k); [right; exact Hin |].
  destruct Hin as [-> | Hin]; [left; reflexivity | right; apply IH; exact Hin].
Qed.

Lemma hdrel_of_all : forall h l, (forall x, In x l -> h <= x) -> HdRel le h l.
Proof.
  intros h l H. destruct l as [| x t]; constructor. apply H. left. reflexivity.
Qed.

Lemma remove_one_sorted : forall k l, Sorted le l -> Sorted le (remove_one k l).
Proof.
  intros k l. induction l as [| h t IH]; cbn; intros Hs; [constructor |].
  inversion Hs as [| ? ? Ht Hrel]; subst.
  destruct (h =? k); [exact Ht |].
  constructor; [apply IH; exact Ht |].
  apply hdrel_of_all. intros x Hx.
  apply (sorted_le_all h t Hs x). eapply remove_one_incl. exact Hx.
Qed.

Lemma canon_remove_one : forall k l, canon (remove_one k l) = remove_one k (canon l).
Proof.
  intros k l. apply sorted_perm_unique.
  - apply canon_sorted.
  - apply remove_one_sorted. apply canon_sorted.
  - eapply perm_trans; [symmetry; apply canon_perm |].
    apply remove_one_perm. apply canon_perm.
Qed.

Lemma NoDup_remove_one : forall k l, NoDup l -> NoDup (remove_one k l).
Proof.
  intros k l Hnd. destruct (in_dec Nat.eq_dec k l) as [Hin | Hn].
  - pose proof (Permutation_NoDup (remove_one_cons_perm k l Hin) Hnd) as Hc.
    apply NoDup_cons_iff in Hc as [_ Hc]. exact Hc.
  - rewrite (remove_one_not_in k l Hn). exact Hnd.
Qed.

(* ── 3. The bag ──────────────────────────────────────────────────────────── *)

Section Bag.
  (* Payloads, patterns, guards and cost authorities. *)
  Variables V P G A : Type.
  (* The payload, the persist flag and the cost authority of the record with
     the source key k. *)
  Variable val : nat -> V.
  Variable persistent : nat -> bool.
  Variable rauth : nat -> A.
  (* The pattern, the guard, the persist flag and the peek flag of the waiter
     with the source key w. *)
  Variable wpat : nat -> P.
  Variable wguard : nat -> G.
  Variable wpersist : nat -> bool.
  Variable wpeek : nat -> bool.
  (* The metered spatial match (SpaceMatcher::bind) and the guard
     (SpaceMatcher::admits). *)
  Variable spatial : P -> V -> bool.
  Variable admits : G -> V -> bool.

  (* ── 3.1 The receive choice (F2) ── *)

  (* The first record in canonical order whose payload the pattern binds. *)
  Definition first_match (p : P) (l : list nat) : option nat :=
    find (fun k => spatial p (val k)) (visit l).

  (* RECEIVE-SCAN: the first spatial match, and the guard checked once on it.
     A refusal parks the receive (None). *)
  Definition receive_choice (p : P) (g : G) (l : list nat) : option nat :=
    match first_match p l with
    | Some k => if admits g (val k) then Some k else None
    | None => None
    end.

  (* F2 bag_consume_first_spatial_match: the receive fires the first record
     in canonical order that its pattern binds, when its guard admits it. No
     earlier record matches. The receive parks when nothing matches, or when
     the guard refuses the first match, whatever the later records (decision
     37: the guard is checked once). *)
  Theorem bag_consume_first_spatial_match : forall p g l,
    (forall k, receive_choice p g l = Some k <->
       exists pre post, visit l = pre ++ k :: post /\
         (forall j, In j pre -> spatial p (val j) = false) /\
         spatial p (val k) = true /\ admits g (val k) = true) /\
    (receive_choice p g l = None <->
       first_match p l = None \/
       exists k, first_match p l = Some k /\ admits g (val k) = false).
  Proof.
    intros p g l. unfold receive_choice. split.
    - intros k. destruct (first_match p l) as [m |] eqn:Hm.
      + unfold first_match in Hm. apply find_some_split in Hm as (pre & post & Hc & Hpre & Hsp).
        destruct (admits g (val m)) eqn:Hadm; split.
        * intros H. injection H as <-. exists pre, post. split; [exact Hc |].
          split; [exact Hpre | split; [exact Hsp | exact Hadm]].
        * intros (pre' & post' & Hc' & Hpre' & Hsp' & Hadm').
          assert (Hfind : find (fun j => spatial p (val j)) (visit l) = Some k).
          { apply find_some_split. exists pre', post'. split; [exact Hc' |].
            split; [exact Hpre' | exact Hsp']. }
          assert (Hfind_m : find (fun j => spatial p (val j)) (visit l) = Some m).
          { apply find_some_split. exists pre, post. split; [exact Hc |].
            split; [exact Hpre | exact Hsp]. }
          rewrite Hfind_m in Hfind. exact Hfind.
        * discriminate.
        * intros (pre' & post' & Hc' & Hpre' & Hsp' & Hadm').
          assert (Hfind : find (fun j => spatial p (val j)) (visit l) = Some k).
          { apply find_some_split. exists pre', post'. split; [exact Hc' |].
            split; [exact Hpre' | exact Hsp']. }
          assert (Hfind_m : find (fun j => spatial p (val j)) (visit l) = Some m).
          { apply find_some_split. exists pre, post. split; [exact Hc |].
            split; [exact Hpre | exact Hsp]. }
          rewrite Hfind_m in Hfind. injection Hfind as ->. rewrite Hadm in Hadm'. discriminate.
      + split; [discriminate |].
        intros (pre & post & Hc & Hpre & Hsp & _).
        assert (Hfind : find (fun j => spatial p (val j)) (visit l) = Some k).
        { apply find_some_split. exists pre, post. split; [exact Hc |].
          split; [exact Hpre | exact Hsp]. }
        unfold first_match in Hm. rewrite Hm in Hfind. discriminate.
    - destruct (first_match p l) as [m |] eqn:Hm; split.
      + destruct (admits g (val m)) eqn:Hadm; [discriminate |].
        intros _. right. exists m. split; [reflexivity | exact Hadm].
      + intros [Hnone | (k & Hk & Hadm)]; [discriminate |].
        injection Hk as ->. rewrite Hadm. reflexivity.
      + intros _. left. reflexivity.
      + intros _. reflexivity.
  Qed.

  Lemma first_match_in : forall p l k, first_match p l = Some k -> In k l.
  Proof.
    intros p l k Hm. unfold first_match, visit in Hm.
    apply find_some_split in Hm as (pre & post & Hc & _ & _).
    eapply Permutation_in; [symmetry; apply canon_perm |].
    rewrite Hc. apply in_or_app. right. left. reflexivity.
  Qed.

  Lemma receive_choice_in : forall p g l k, receive_choice p g l = Some k -> In k l.
  Proof.
    intros p g l k H. unfold receive_choice in H.
    destruct (first_match p l) as [m |] eqn:Hm; [| discriminate].
    destruct (admits g (val m)); [| discriminate].
    injection H as <-. eapply first_match_in. exact Hm.
  Qed.

  (* ── 3.2 The staged operations and their effect on the records ── *)

  (* A staged operation of the driver, with the introduction authority that
     its callback registers. *)
  Inductive op :=
  | Store (k : nat) (a : A)                (* Cx::store_on of the record k *)
  | Park (c : nat) (a : A)                 (* Cx::park of the continuation c *)
  | FireIn (w : nat) (k : nat) (a : A)     (* Cx::fire_offer_on: k fires w *)
  | FireSt (c : nat) (k : nat) (a : A).    (* Cx::fire_receive_group: c takes k *)

  (* The records of D after the operation [o] on the records [rs]. A store
     puts its record at the front, as put_datum does. The incoming datum of a
     produce-side fire is never stored. A consume-side fire removes the
     record that it binds unless the record is persistent, also for a peek
     (decision 45, Rule E). *)
  Definition on_records (o : op) (rs : list nat) : list nat :=
    match o with
    | Store k _ => k :: rs
    | FireSt _ k _ => if persistent k then rs else remove_one k rs
    | Park _ _ => rs
    | FireIn _ _ _ => rs
    end.

  Definition apply_ops (ops : list op) (rs : list nat) : list nat :=
    fold_left (fun acc o => on_records o acc) ops rs.

  (* F5 bag_persistent_match_not_removed: a fire on a persistent record
     leaves it (a COMM never removes a persistent record). *)
  Theorem bag_persistent_match_not_removed : forall c k a rs,
    persistent k = true -> on_records (FireSt c k a) rs = rs.
  Proof.
    intros c k a rs Hp. cbn [on_records]. rewrite Hp. reflexivity.
  Qed.

  (* The record that the operation [o] adds, and the record that it takes
     from [rs]. *)
  Definition added (o : op) (j : nat) : nat :=
    match o with
    | Store k _ => if k =? j then 1 else 0
    | _ => 0
    end.

  Definition taken (o : op) (rs : list nat) (j : nat) : nat :=
    match o with
    | FireSt _ k _ => if negb (persistent k) && (k =? j) && (0 <? count j rs) then 1 else 0
    | _ => 0
    end.

  (* F3 bag_multiplicity_preserved: for every record, a store adds exactly
     one occurrence, a fire removes exactly the one non-persistent record that
     it binds, and no other operation changes the records. *)
  Theorem bag_multiplicity_preserved : forall o rs j,
    count j (on_records o rs) + taken o rs j = count j rs + added o j.
  Proof.
    intros [k a | c a | w k a | c k a] rs j; cbn [on_records taken added].
    - cbn [count]. lia.
    - lia.
    - lia.
    - destruct (persistent k); cbn [negb andb].
      + lia.
      + pose proof (count_remove_one k rs j). lia.
  Qed.

  (* The fire of a non-persistent record removes one occurrence of it. *)
  Lemma on_records_fire_transient : forall c k a rs,
    persistent k = false -> on_records (FireSt c k a) rs = remove_one k rs.
  Proof. intros c k a rs Hp. unfold on_records. rewrite Hp. reflexivity. Qed.

  (* F4 bag_removal_keeps_equal_duplicate: of two or more equal records, a
     fire removes exactly one and leaves the others. *)
  Theorem bag_removal_keeps_equal_duplicate : forall c k a rs,
    persistent k = false -> 2 <= count k rs ->
    count k (on_records (FireSt c k a) rs) + 1 = count k rs /\
    1 <= count k (on_records (FireSt c k a) rs).
  Proof.
    intros c k a rs Hp H2. rewrite (on_records_fire_transient c k a rs Hp).
    pose proof (count_remove_one_in k rs ltac:(lia)) as Hc. split; lia.
  Qed.

  (* ── 3.3 OFFER-SCAN and the host send ── *)

  (* The waiter [w] takes the datum [x]: its pattern binds the payload and
     its guard admits it. *)
  Definition fits (x w : nat) : bool := spatial (wpat w) (val x) && admits (wguard w) (val x).

  (* The end of a plan: the element settled (stored, or spent by a fire), or
     it is still in flight and its operation re-issues (A10, I-21). *)
  Inductive ending := Settled | Reissue.

  (* OFFER-SCAN of the datum [x] with the introduction [a] over the waiters
     [cands] in canonical order. A waiter that does not fit is skipped, and a
     guard refusal moves to the next waiter (decision 37). A fire that leaves
     [x] in flight continues with the next waiter (decision 30). The datum is
     spent when it is not persistent and the waiter does not peek, and the
     waiter stays when it is persistent. *)
  Fixpoint offer_pass (cands : list nat) (x : nat) (a : A) (rounds : nat) : list op * ending :=
    match cands with
    | [] => ([Store x a], Settled)
    | w :: rest =>
        if fits x w then
          if negb (persistent x) && negb (wpeek w) then ([FireIn w x a], Settled)
          else if wpersist w then ([FireIn w x a], Reissue)
          else match rounds with
               | 0 => ([FireIn w x a], Reissue)
               | S r => let (ops, e) := offer_pass rest x a r in (FireIn w x a :: ops, e)
               end
        else offer_pass rest x a rounds
    end.

  Definition offer (ws : list nat) (x : nat) (a : A) (rounds : nat) : list op * ending :=
    offer_pass (canon ws) x a rounds.

  (* The host program of one send of [x] with the introduction [a] on the
     waiters [ws]: the host produce fires the first waiter in canonical order
     that fits, and the reducer sends [x] again while a fire leaves it in
     flight. Each call scans every remaining waiter from the first one. *)
  Fixpoint host_send (fuel : nat) (ws : list nat) (x : nat) (a : A) : list op :=
    match fuel with
    | 0 => []
    | S f =>
        match find (fits x) (canon ws) with
        | None => [Store x a]
        | Some w =>
            if negb (persistent x) && negb (wpeek w) then [FireIn w x a]
            else if wpersist w then [FireIn w x a]
            else FireIn w x a :: host_send f (remove_one w ws) x a
        end
    end.

  (* The single pass of OFFER-SCAN over the tested prefix equals the re-scans
     of the host: a waiter that failed the datum fails it again, and the
     removal of a fired waiter keeps the order of the others. *)
  Lemma offer_pass_is_host_send : forall cands x a rounds pre ws fuel,
    canon ws = pre ++ cands ->
    (forall j, In j pre -> fits x j = false) ->
    NoDup ws ->
    length cands < fuel ->
    snd (offer_pass cands x a rounds) = Settled ->
    fst (offer_pass cands x a rounds) = host_send fuel ws x a.
  Proof.
    induction cands as [| w rest IH]; intros x a rounds pre ws fuel Hcanon Hpre Hnd Hfuel Hset.
    - destruct fuel as [| f]; [cbn in Hfuel; lia |].
      cbn [offer_pass fst host_send]. rewrite Hcanon, app_nil_r.
      rewrite (find_none_all (fits x) pre Hpre). reflexivity.
    - destruct fuel as [| f]; [cbn in Hfuel; lia |].
      cbn [offer_pass] in Hset |- *.
      destruct (fits x w) eqn:Hfit.
      + assert (Hfind : find (fits x) (canon ws) = Some w).
        { rewrite Hcanon, (find_app_skip (fits x) pre (w :: rest) Hpre). cbn.
          rewrite Hfit. reflexivity. }
        cbn [host_send]. rewrite Hfind.
        destruct (negb (persistent x) && negb (wpeek w)); [reflexivity |].
        destruct (wpersist w); [cbn in Hset; discriminate |].
        destruct rounds as [| r]; [cbn in Hset; discriminate |].
        destruct (offer_pass rest x a r) as [ops e] eqn:Hrest.
        cbn in Hset |- *. f_equal.
        assert (Hnd_canon : NoDup (pre ++ w :: rest)).
        { rewrite <- Hcanon. exact (Permutation_NoDup (canon_perm ws) Hnd). }
        assert (Hin : ~ In w pre).
        { intros Hw. apply (NoDup_remove_2 pre rest w Hnd_canon). apply in_or_app. left. exact Hw. }
        replace ops with (fst (offer_pass rest x a r)) by (rewrite Hrest; reflexivity).
        apply (IH x a r pre (remove_one w ws) f).
        * rewrite canon_remove_one, Hcanon. apply remove_one_app. exact Hin.
        * exact Hpre.
        * apply NoDup_remove_one. exact Hnd.
        * cbn in Hfuel. lia.
        * rewrite Hrest. exact Hset.
      + apply (IH x a rounds (pre ++ [w]) ws (S f)).
        * rewrite Hcanon, <- app_assoc. reflexivity.
        * intros j Hj. apply in_app_or in Hj as [Hj | [<- | []]];
            [apply Hpre; exact Hj | exact Hfit].
        * exact Hnd.
        * cbn in Hfuel. lia.
        * exact Hset.
  Qed.

  (* An offer that no waiter takes stores the datum. *)
  Lemma offer_pass_none : forall cands x a rounds,
    (forall j, In j cands -> fits x j = false) ->
    offer_pass cands x a rounds = ([Store x a], Settled).
  Proof.
    induction cands as [| w rest IH]; intros x a rounds H; cbn; [reflexivity |].
    rewrite (H w (or_introl eq_refl)). apply IH. intros j Hj. apply H. right. exact Hj.
  Qed.

  (* ── 3.4 RECEIVE-SCAN with a peek, and RESTORE-SCAN (Rule E) ── *)

  (* A peek receive of the continuation [c] with the pattern [p], the guard
     [g] and the introduction [a_op], over the records [rs] and the waiters
     [ws]. The fire removes a non-persistent record, and RESTORE-SCAN offers
     it again with its own authority [rauth k], sharing the rounds of the
     receive. *)
  Definition peek_plan (c : nat) (p : P) (g : G) (rs ws : list nat) (rounds : nat) (a_op : A)
    : list op * ending :=
    match receive_choice p g rs with
    | None => ([Park c a_op], Settled)
    | Some k =>
        if persistent k then ([FireSt c k a_op], Settled)
        else let (ops, e) := offer ws k (rauth k) rounds in (FireSt c k a_op :: ops, e)
    end.

  (* The host program of a peek receive: the host consume takes the first
     spatial match and checks the guard once (receive_choice, F2), its COMM
     removes a non-persistent record, and the reducer sends the removed record
     again with the cost authority of the record (produce_peeks). *)
  Definition host_peek (fuel : nat) (c : nat) (p : P) (g : G) (rs ws : list nat) (a_op : A)
    : list op :=
    match receive_choice p g rs with
    | None => [Park c a_op]
    | Some k => FireSt c k a_op :: (if persistent k then [] else host_send fuel ws k (rauth k))
    end.

  (* Rule E bag_peek_restore_is_host_second_send: a peek receive that settles
     stages exactly the host program: the fire, then the second send of the
     removed record, which is offered to the waiters first and registers the
     authority of the record. When no waiter takes the record, the record
     is stored again, so the peek leaves the multiset of records as it was. *)
  Theorem bag_peek_restore_is_host_second_send : forall c p g rs ws rounds a_op fuel,
    NoDup ws -> length ws < fuel ->
    snd (peek_plan c p g rs ws rounds a_op) = Settled ->
    fst (peek_plan c p g rs ws rounds a_op) = host_peek fuel c p g rs ws a_op /\
    (forall k, receive_choice p g rs = Some k -> persistent k = false ->
       (forall w, In w ws -> fits k w = false) ->
       count k (apply_ops (fst (peek_plan c p g rs ws rounds a_op)) rs) = count k rs).
  Proof.
    intros c p g rs ws rounds a_op fuel Hnd Hfuel Hset. split.
    - unfold peek_plan, host_peek in *.
      destruct (receive_choice p g rs) as [k |]; [| reflexivity].
      destruct (persistent k); [reflexivity |].
      unfold offer in *.
      destruct (offer_pass (canon ws) k (rauth k) rounds) as [ops e] eqn:Hoffer.
      cbn in Hset |- *. f_equal.
      replace ops with (fst (offer_pass (canon ws) k (rauth k) rounds)) by (rewrite Hoffer; reflexivity).
      apply (offer_pass_is_host_send (canon ws) k (rauth k) rounds [] ws fuel).
      + reflexivity.
      + intros j [].
      + exact Hnd.
      + rewrite <- (Permutation_length (canon_perm ws)). exact Hfuel.
      + rewrite Hoffer. exact Hset.
    - intros k Hk Hp Hnone. unfold peek_plan. rewrite Hk, Hp. unfold offer.
      rewrite (offer_pass_none (canon ws) k (rauth k) rounds).
      + cbn [fst apply_ops fold_left]. rewrite (on_records_fire_transient c k a_op rs Hp).
        cbn [on_records count]. rewrite Nat.eqb_refl.
        assert (Hin : 1 <= count k rs) by (apply count_in; eapply receive_choice_in; exact Hk).
        pose proof (count_remove_one_in k rs Hin) as Hc. cbn [plus]. lia.
      + intros j Hj. apply Hnone. eapply Permutation_in; [symmetry; apply canon_perm | exact Hj].
  Qed.
End Bag.

(* ── 4. Metering of the scan driver (decision 42) ────────────────────────── *)

(* The metering rule of DR-88, restated from AgentReadCoverage.v: a trace of
   reservations and reads, covered when every prefix reads at most what it
   reserved. *)
Inductive event := Reserve (units : nat) | Read (units : nat).

Definition reserved_units (e : event) : nat :=
  match e with Reserve units => units | Read _ => 0 end.

Definition read_units (e : event) : nat :=
  match e with Read units => units | Reserve _ => 0 end.

Definition total (project : event -> nat) (trace : list event) : nat :=
  fold_right Nat.add 0 (map project trace).

Definition covered (trace : list event) : Prop :=
  forall prefix suffix, trace = prefix ++ suffix ->
    total read_units prefix <= total reserved_units prefix.

Lemma total_app : forall project left right,
  total project (left ++ right) = total project left + total project right.
Proof.
  intros project left right. unfold total. rewrite map_app, fold_right_app.
  induction left as [| e rest IH]; cbn; [reflexivity |].
  unfold total in IH. rewrite IH. lia.
Qed.

Lemma covered_app : forall left right,
  covered left -> total read_units left <= total reserved_units left -> covered right ->
  covered (left ++ right).
Proof.
  intros left right Hl Hbal Hr prefix suffix Hsplit.
  apply app_eq_app in Hsplit as [middle [[Hleft Hsuffix] | [Hprefix Hright]]].
  - exact (Hl prefix middle Hleft).
  - subst prefix. pose proof (Hr middle suffix Hright) as Hm. rewrite !total_app. lia.
Qed.

(* A self-metered step: reserve the units, then read them. *)
Definition step (units : nat) : list event := [Reserve units; Read units].

Lemma step_covered : forall units, covered (step units).
Proof.
  intros units prefix suffix Hsplit. unfold step in Hsplit.
  destruct prefix as [| a [| b [| c rest]]]; cbn in Hsplit.
  - cbn. lia.
  - injection Hsplit as <- _. cbn. lia.
  - injection Hsplit as <- <- _. cbn. lia.
  - injection Hsplit as <- <- Hnil. destruct rest; discriminate.
Qed.

Lemma step_balanced : forall units,
  total read_units (step units) <= total reserved_units (step units).
Proof. intros units. cbn. lia. Qed.

Lemma steps_covered : forall units,
  covered (flat_map step units) /\
  total read_units (flat_map step units) <= total reserved_units (flat_map step units).
Proof.
  induction units as [| u rest [IHc IHb]]; cbn [flat_map].
  - split; [intros prefix suffix Hs; destruct prefix; [cbn; lia | discriminate] | cbn; lia].
  - split.
    + apply covered_app; [apply step_covered | apply step_balanced | exact IHc].
    + rewrite !total_app. pose proof (step_balanced u). lia.
Qed.

(* The parts of one scan of the driver, in their units: the canonical order,
   the payload decode and the metered match of each tested record, the guard
   of the receive, the copy of the selected record, the copies of the staged
   operation, and the prepaid outcome. *)
Record scan := {
  order : nat;
  tested : list (nat * nat);
  guard : nat;
  selection : nat;
  staging : list nat;
  outcome : nat
}.

(* record::unwrap_datum_metered reserves the read of the envelope and the
   copy of the payload, then decodes. *)
Definition decode (units : nat) : list event := step units.

(* The metered matcher reserves each read before it makes it (DR-88). *)
Definition matched (units : nat) : list event := step units.

(* The trace of a scan: each part reserves its work before it makes it, and
   the outcome is reserved at staging and read after the commit
   (prepay_continuation_unwrap, prepay_outcome). *)
Definition scan_trace (s : scan) : list event :=
  step (order s)
  ++ flat_map (fun dm => decode (fst dm) ++ matched (snd dm)) (tested s)
  ++ step (guard s) ++ step (selection s) ++ flat_map step (staging s)
  ++ [Reserve (outcome s)] ++ [Read (outcome s)].

(* decision 42 scan_trace_covered: every prefix of the trace of a scan of the
   driver, the reads after the commit included, reads at most what it
   reserved. So a budget below the reservations of an operation is exceeded
   by a reservation before the commit, and the operation has no effect. *)
Theorem scan_trace_covered : forall s, covered (scan_trace s).
Proof.
  intros s. unfold scan_trace.
  assert (Htested : covered (flat_map (fun dm => decode (fst dm) ++ matched (snd dm)) (tested s)) /\
            total read_units (flat_map (fun dm => decode (fst dm) ++ matched (snd dm)) (tested s))
            <= total reserved_units (flat_map (fun dm => decode (fst dm) ++ matched (snd dm)) (tested s))).
  { induction (tested s) as [| [d m] rest [IHc IHb]]; cbn [flat_map fst snd].
    - split; [intros prefix suffix Hs; destruct prefix; [cbn; lia | discriminate] | cbn; lia].
    - change (decode d) with (step d). change (matched m) with (step m).
      rewrite <- app_assoc. split.
      + apply covered_app; [apply step_covered | apply step_balanced |].
        apply covered_app; [apply step_covered | apply step_balanced | exact IHc].
      + rewrite !total_app. pose proof (step_balanced d). pose proof (step_balanced m). lia. }
  destruct Htested as [Htc Htb].
  destruct (steps_covered (staging s)) as [Hsc Hsb].
  apply covered_app; [apply step_covered | apply step_balanced |].
  apply covered_app; [exact Htc | exact Htb |].
  apply covered_app; [apply step_covered | apply step_balanced |].
  apply covered_app; [apply step_covered | apply step_balanced |].
  apply covered_app; [exact Hsc | exact Hsb |].
  change ([Reserve (outcome s)] ++ [Read (outcome s)]) with (step (outcome s)).
  apply step_covered.
Qed.

Print Assumptions bag_visit_order_canonical.
Print Assumptions bag_consume_first_spatial_match.
Print Assumptions bag_persistent_match_not_removed.
Print Assumptions bag_multiplicity_preserved.
Print Assumptions bag_removal_keeps_equal_duplicate.
Print Assumptions bag_peek_restore_is_host_second_send.
Print Assumptions scan_trace_covered.
