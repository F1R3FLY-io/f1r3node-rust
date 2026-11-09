(* ═══════════════════════════════════════════════════════════════════════════
   SetDiscipline.v — the set discipline under the scan driver
   (P5a: F3, F5 to F10, F14, F20, and decisions 37, 41, 42, 43 and 45)

   A set collection keeps every record on its data channel D(sid,k), and its
   view removes the duplicates. Two records are members of one class when the
   set keys of their payloads are equal (F5). The plans are the scan driver of
   P5 (spaces/disciplines/scan.rs) with the class key of the set
   (spaces/disciplines/set.rs):
   - A receive takes the first record in the canonical order of DR-75 whose
     payload its pattern binds, and it checks its guard once (decision 37). A
     receive that does not peek fires one consume over [D; n] with every
     stored member of the class, and it dispatches the result of the
     representative only (F3, F20).
   - A send fires the first waiter in canonical order that takes its datum.
     When the waiter does not peek, the send retires the stored
     non-persistent members of the class of the datum with single-channel
     K_retire COMMs (decision 37, readiness review S3).
   - A peek binds the representative alone. Its fire removes a non-persistent
     representative, and RESTORE-SCAN sends it again (decision 45, Rule E).
   - The driver compares two keys by their metered hashes, and the full keys
     only on equal hashes (DR-90). Every read and copy is reserved before it
     is made (decision 42).

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md, A.7 (the set row,
     OFFER-SCAN, RECEIVE-SCAN, RESTORE-SCAN, RETIRE-CLASS and the defined
     effects), A.12 (the set row), A.13 (this module: 8 headlines), C.1. The P5
     design v2, 2.3 (the set key and the set waiters) and 2.8.1 (F6 to F10).
   - Plan: decisions 37, 39 (F3, F5, F14, F20), 41, 42, 43 and 45.
   - Code: rholang/src/rust/interpreter/spaces/disciplines/set.rs and scan.rs
     at the P5 wave W2 checkpoint bc43b8653 (scan.rs) and the SET batch
     (set.rs). The line numbers below are the line numbers of those files.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   be, u64_be, LENGTH_BYTES set.rs:74 and 223-227 (u64_be, the 8 big-endian
                            bytes of u64::to_be_bytes)
   frame, key_of_encodings,
   canonical_bytes, set_key set.rs:204-222 (set_key: the count field, then
                            the length field and the bytes of each element)
                            and 188-208 (canonical_bytes: bincode of the
                            sorted Par, metered sort and encode)
   rkey, same, class_val    set.rs:160-165 (ScanDiscipline::class_key) and
                            scan.rs:878-893 (same_class)
   canon, first_match       space_store_handle.rs canonical_data_order
                            (DR-75) and scan.rs:564-584 (first_bound_record)
   view, dedup              the classes in the order of their first members:
                            scan.rs:591-647 (class_of: the representative
                            is the first member in canonical order)
   receive_choice           scan.rs:229-321 (receive: the first spatial
                            match, Cx::admits once, then park)
   class_of, remove_all     scan.rs:591-647 (class_of) and 643-670
                            (fire_class: the consume over [D; n] that
                            removes each non-persistent member)
   staged, class_fire,
   retire_fire, stages      generic.rs fire_receive_group and retire, and the
                            staging rules of space_store_handle.rs
                            (check_arity, canonical_group_key,
                            check_lined_up, check_stored_candidates,
                            claim_positions); scan.rs:652-679 and 788-873
   op, on_records           the staged operations of the driver (Cx::store_on,
                            Cx::park, Cx::fire_offer_on,
                            Cx::fire_receive_group, Cx::retire)
   retire_class             scan.rs:788-873 (retire_class: K_retire of each
                            stored non-persistent member in canonical order)
   fits, offer_pass         scan.rs:408-493 (offer_scan with
                            first_admitting_waiter at 519-550 and
                            retiring_of at 491-513)
   peek_group, peek_plan    scan.rs:229-321 (receive: a peek binds [r]),
                            694-704 (restore_in) and 715-747 (restore_scan)
   same_class               scan.rs:878-893 (same_class: metered_hash, then
                            the full compare only on equal hashes)
   key_trace                set.rs:204-250 (the reserved buffers, the
                            metered sort and encode) and scan.rs:878-893
                            (the reserved hash and comparisons)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - Bytes are naturals. The key model is exact for the framing: the count
     and each length are u64 fields of 8 big-endian bytes, injective below
     256^8, which holds for usize lengths. The sort and the encode of an
     element are section variables: F6 holds for every sort and every
     encode, and its second half takes the injectivity of the encode of
     sorted terms (bincode) as an explicit premise.
   - A record is its source key, as in DataCollection.v: equal source keys
     are byte-identical records, and a list of keys is the multiset of
     records. The canonical order sorts the keys.
   - val k is the sorted payload of the record k, and vkey is the set key of
     a sorted payload. The section hypothesis vkey_injective is F6 with an
     injective encode: equal keys are equal sorted payloads. The matcher sees
     the payload through val, so the members of one class bind a pattern
     alike. The Rust driver does not assume this: class_of binds each member
     again and fails closed with Inconsistent when a member does not bind
     (scan.rs:634-638).
   - A set has no counter, so every ord is 0 (decision 43), and the model
     needs no ord. rounds counts the fires that FIRE_ROUND_CAP still allows
     after the next fire (I-21).
   - The hash of the membership test is any function: DR-90 holds without a
     property of the hash.
   - The metering model restates the rule of AgentReadCoverage.v (DR-88 of
     feature/cost-accounted-rho), as DataCollection.v does: a self-metered
     step reserves its units and then reads them, and a trace is covered when
     every prefix reads at most what it reserved.
     Changed by decision 96 (decision 89, DR-131): AgentReadCoverage.v is
     deleted, and section 8 goes with the host-work metering, as in
     DataCollection.v.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. set_key_injective                  F6: the key of two payloads is
                                          equal exactly when their canonical
                                          element encodings are equal, and,
                                          with an injective encode, exactly
                                          when their sorted elements are equal
    2. set_view_dedups_equal_pars         F7: one representative for each
                                          class, the first member in
                                          canonical order, independent of the
                                          list order; the first spatial match
                                          is a representative; a class is
                                          one sorted payload
    3. set_consume_retires_class          F8: a receive that does not peek
                                          binds every member, the
                                          representative first, and removes
                                          every non-persistent member
    4. set_retire_comm_lines_up           F9: the consume over [D; n] and each
                                          K_retire meet the staging rules of
                                          the substrate, with one and no
                                          dispatched result
    5. set_produce_fire_retires_stored_class
                                          F10: RETIRE-CLASS clears the class,
                                          and an offer clears the class after
                                          a fire of a waiter that does not
                                          peek, and a peek fire retires
                                          nothing
    6. set_peek_restores_representative_only
                                          Rule E: a peek binds the
                                          representative alone and sends it
                                          again, and only a fire of that
                                          second send retires the class
    7. set_membership_full_compare_on_equal_hash
                                          DR-90: the membership test answers
                                          key equality for every hash, and it
                                          compares the full keys exactly on
                                          equal hashes
    8. set_key_trace_covered              decision 42: the trace of a set key
                                          and of a membership test reads after
                                          it reserves
                                          Decision 96 (decision 89, DR-131)
                                          disables this headline with the
                                          host-work metering.

   The negative controls of the SET batch (16, fragment
   target/verification/reified-rspaces/p5/fragments/SET/rocq/set_controls.py)
   mutate one rule each: the length fields, the sort, the dedup of the view,
   the canonical order of the view, the class of a consume, the persistent
   members of a class fire, a repeated representative, a dispatched retire,
   a dispatched class, RETIRE-CLASS, a retire after a peek fire, the group of
   a peek, the second send of a peek, a membership test without the full
   compare, a full compare on unequal hashes, and a key buffer that is
   filled before it is reserved.

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables and the section hypothesis vkey_injective are parameters of the
   closed theorems. The file imports only the Rocq standard library.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

(* ── 1. The set key (F5, F6) ─────────────────────────────────────────────── *)

(* The [w] big-endian bytes of [n]: the bytes of u64::to_be_bytes for w = 8. *)
Fixpoint be (w n : nat) : list nat :=
  match w with
  | 0 => []
  | S w' => be w' (n / 256) ++ [n mod 256]
  end.

(* The bytes of one length field of a set key (set.rs LENGTH_BYTES). *)
Definition LENGTH_BYTES : nat := 8.

(* The big-endian u64 field of [n] (set.rs u64_be). *)
Definition u64_be (n : nat) : list nat := be LENGTH_BYTES n.

(* The values that a u64 field holds. *)
Definition U64_BOUND : nat := 256 ^ LENGTH_BYTES.

Lemma be_length : forall w n, length (be w n) = w.
Proof.
  induction w as [| w IH]; intros n; cbn [be]; [reflexivity |].
  rewrite length_app, IH. cbn. lia.
Qed.

Lemma be_injective : forall w n m, n < 256 ^ w -> m < 256 ^ w -> be w n = be w m -> n = m.
Proof.
  induction w as [| w IH]; intros n m Hn Hm Heq.
  - cbn in Hn, Hm. lia.
  - cbn [be] in Heq. apply app_inj_tail in Heq as [Hhigh Hlow].
    rewrite Nat.pow_succ_r' in Hn, Hm.
    assert (Hn' : n / 256 < 256 ^ w) by (apply Nat.Div0.div_lt_upper_bound; exact Hn).
    assert (Hm' : m / 256 < 256 ^ w) by (apply Nat.Div0.div_lt_upper_bound; exact Hm).
    pose proof (IH _ _ Hn' Hm' Hhigh) as Hq.
    rewrite (Nat.div_mod_eq n 256), (Nat.div_mod_eq m 256), Hq, Hlow. reflexivity.
Qed.

Lemma u64_be_length : forall n, length (u64_be n) = LENGTH_BYTES.
Proof. intros n. apply be_length. Qed.

Lemma u64_be_injective : forall n m, n < U64_BOUND -> m < U64_BOUND -> u64_be n = u64_be m -> n = m.
Proof. intros n m. apply be_injective. Qed.

(* The frame of one element encoding: its length field, then its bytes. *)
Definition frame (b : list nat) : list nat := u64_be (length b) ++ b.

(* The set key over the element encodings [bs]: the count field, then the frames
   (set.rs set_key). *)
Definition key_of_encodings (bs : list (list nat)) : list nat :=
  u64_be (length bs) ++ flat_map frame bs.

(* The count and each length fit their u64 fields (usize lengths). *)
Definition fits_u64 (bs : list (list nat)) : Prop :=
  length bs < U64_BOUND /\ Forall (fun b => length b < U64_BOUND) bs.

Lemma app_eq_same_length : forall (a b c d : list nat),
  length a = length b -> a ++ c = b ++ d -> a = b /\ c = d.
Proof.
  induction a as [| x a IH]; intros [| y b] c d Hlen Heq; cbn in *; try discriminate.
  - split; [reflexivity | exact Heq].
  - injection Heq as -> Heq. injection Hlen as Hlen.
    destruct (IH b c d Hlen Heq) as [-> ->]. split; reflexivity.
Qed.

(* The frames of encodings of equal count are equal only for equal encodings: each length
   field tells where its encoding ends. *)
Lemma frames_injective : forall bs cs,
  Forall (fun b => length b < U64_BOUND) bs ->
  Forall (fun c => length c < U64_BOUND) cs ->
  length bs = length cs ->
  flat_map frame bs = flat_map frame cs -> bs = cs.
Proof.
  induction bs as [| b bs IH]; intros [| c cs] Hb Hc Hlen Heq; cbn in Hlen; try discriminate.
  - reflexivity.
  - inversion Hb as [| ? ? Hbl Hbs]; subst. inversion Hc as [| ? ? Hcl Hcs]; subst.
    cbn [flat_map] in Heq. unfold frame in Heq. rewrite <- !app_assoc in Heq.
    apply app_eq_same_length in Heq as [Hu Hrest]; [| rewrite !u64_be_length; reflexivity].
    apply u64_be_injective in Hu; [| exact Hbl | exact Hcl].
    apply app_eq_same_length in Hrest as [-> Hrest]; [| exact Hu].
    f_equal. apply IH; [exact Hbs | exact Hcs | lia | exact Hrest].
Qed.

Lemma key_of_encodings_injective : forall bs cs,
  fits_u64 bs -> fits_u64 cs -> key_of_encodings bs = key_of_encodings cs -> bs = cs.
Proof.
  intros bs cs [Hn Hb] [Hm Hc] Heq. unfold key_of_encodings in Heq.
  apply app_eq_same_length in Heq as [Hu Hrest]; [| rewrite !u64_be_length; reflexivity].
  apply u64_be_injective in Hu; [| exact Hn | exact Hm].
  exact (frames_injective bs cs Hb Hc Hu Hrest).
Qed.

(* The length fields are needed: two lists of encodings with one concatenation collide
   without them, and the key separates them. *)
Lemma length_framing_needed :
  concat [[1]; [1; 2]] = concat [[1; 1]; [2]] /\ [[1]; [1; 2]] <> [[1; 1]; [2]] /\
  key_of_encodings [[1]; [1; 2]] <> key_of_encodings [[1; 1]; [2]].
Proof.
  split; [reflexivity |]. split; [discriminate |].
  intros H. unfold key_of_encodings, frame in H. cbn [flat_map length] in H.
  rewrite <- !app_assoc in H. apply app_eq_same_length in H as [_ H]; [| reflexivity].
  apply app_eq_same_length in H as [H _]; [| rewrite !u64_be_length; reflexivity].
  vm_compute in H. discriminate H.
Qed.

Lemma map_injective : forall (A B : Type) (f : A -> B),
  (forall x y, f x = f y -> x = y) -> forall l l', map f l = map f l' -> l = l'.
Proof.
  intros A B f Hf. induction l as [| x l IH]; intros [| y l'] H; cbn in H; try discriminate.
  - reflexivity.
  - injection H as Hxy Hl. f_equal; [apply Hf; exact Hxy | apply IH; exact Hl].
Qed.

Section SetKey.
  (* An element of a payload (a Par), its sort (ParSortMatcher::sort_match(_).term) and its
     bincode encoding. *)
  Variable T : Type.
  Variable srt : T -> T.
  Variable enc : T -> list nat.

  (* The canonical bytes of one element (set.rs canonical_bytes). *)
  Definition canonical_bytes (p : T) : list nat := enc (srt p).

  (* The F5 set key of the payload [ps] (set.rs set_key). *)
  Definition set_key (ps : list T) : list nat := key_of_encodings (map canonical_bytes ps).

  Lemma canonical_map : forall ps, map canonical_bytes ps = map enc (map srt ps).
  Proof. intros ps. rewrite map_map. reflexivity. Qed.

  (* F6 set_key_injective: the key of two payloads is equal exactly when their canonical
     element encodings are equal. With an injective encoding of sorted terms (bincode), the key
     is equal exactly when the sorted elements are equal. *)
  Theorem set_key_injective : forall ps qs,
    fits_u64 (map canonical_bytes ps) -> fits_u64 (map canonical_bytes qs) ->
    (set_key ps = set_key qs <-> map canonical_bytes ps = map canonical_bytes qs) /\
    ((forall x y, enc x = enc y -> x = y) ->
       (set_key ps = set_key qs <-> map srt ps = map srt qs)).
  Proof.
    intros ps qs Hp Hq. split.
    - split.
      + intros H. exact (key_of_encodings_injective _ _ Hp Hq H).
      + intros H. unfold set_key. rewrite H. reflexivity.
    - intros Henc. split.
      + intros H. pose proof (key_of_encodings_injective _ _ Hp Hq H) as Hb.
        rewrite (canonical_map ps), (canonical_map qs) in Hb.
        exact (map_injective _ _ enc Henc _ _ Hb).
      + intros H. unfold set_key. rewrite (canonical_map ps), (canonical_map qs), H.
        reflexivity.
  Qed.
End SetKey.

(* ── 2. Lists of records and the canonical order (DR-75, decision 41) ─────── *)

(* The insertion of the key [k] into a sorted list. *)
Fixpoint insert_key (k : nat) (l : list nat) : list nat :=
  match l with
  | [] => [k]
  | h :: t => if k <=? h then k :: h :: t else h :: insert_key k t
  end.

(* The canonical order of the records [l]: their source keys in ascending order
   (space_store_handle.rs canonical_data_order). Equal source keys are byte-identical
   records, so a sort of the keys is the canonical order. *)
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

Lemma sorted_le_all : forall a l, Sorted le (a :: l) -> forall x, In x l -> a <= x.
Proof.
  intros a l. revert a. induction l as [| b t IH]; intros a Hs x Hin; [destruct Hin |].
  inversion Hs as [| ? ? Ht Hrel]; subst. inversion Hrel; subst.
  destruct Hin as [-> | Hin]; [assumption |].
  specialize (IH b Ht x Hin). lia.
Qed.

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

(* The number of occurrences of the key [j] in [l]. *)
Fixpoint count (j : nat) (l : list nat) : nat :=
  match l with
  | [] => 0
  | h :: t => (if h =? j then 1 else 0) + count j t
  end.

(* The removal of the first occurrence of [k]: a removal by record identity at a claimed
   position (space_store_handle.rs claim_positions). *)
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

Lemma remove_one_cons_perm : forall k l, In k l -> Permutation l (k :: remove_one k l).
Proof.
  intros k l. induction l as [| h t IH]; cbn; intros Hin; [destruct Hin |].
  destruct (Nat.eqb_spec h k) as [-> | Hhk]; [reflexivity |].
  destruct Hin as [Hin | Hin]; [contradiction |].
  eapply perm_trans; [apply perm_skip; exact (IH Hin) | apply perm_swap].
Qed.

Lemma count_in : forall j l, In j l -> 1 <= count j l.
Proof.
  intros j l. induction l as [| h t IH]; cbn; intros Hin; [destruct Hin |].
  destruct Hin as [-> | Hin]; [rewrite Nat.eqb_refl; lia |].
  specialize (IH Hin). lia.
Qed.

Lemma count_zero_not_in : forall j l, count j l = 0 -> ~ In j l.
Proof. intros j l H Hin. pose proof (count_in j l Hin). lia. Qed.

Lemma count_perm : forall j l l', Permutation l l' -> count j l = count j l'.
Proof.
  intros j l l' Hp. induction Hp as [| x l l' Hp IH | x y l | l l' l'' Hp1 IH1 Hp2 IH2];
    cbn [count]; lia.
Qed.

Lemma count_canon : forall j l, count j (canon l) = count j l.
Proof. intros j l. symmetry. apply count_perm, canon_perm. Qed.

Lemma count_filter : forall (f : nat -> bool) j l,
  count j (filter f l) = if f j then count j l else 0.
Proof.
  intros f j l. induction l as [| h t IH]; cbn [filter count].
  - destruct (f j); reflexivity.
  - destruct (Nat.eqb_spec h j) as [-> | Hhj].
    + destruct (f j) eqn:Hf; cbn [count]; rewrite ?Nat.eqb_refl, IH, ?Hf; lia.
    + destruct (f h); cbn [count]; [destruct (Nat.eqb_spec h j); [contradiction |] |];
        rewrite IH; destruct (f j); lia.
Qed.

Lemma in_canon : forall x l, In x (canon l) <-> In x l.
Proof.
  intros x l. split; intros H.
  - eapply Permutation_in; [symmetry; apply canon_perm | exact H].
  - eapply Permutation_in; [apply canon_perm | exact H].
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

Lemma filter_sorted : forall (f : nat -> bool) l, Sorted le l -> Sorted le (filter f l).
Proof.
  intros f l Hs. apply StronglySorted_Sorted.
  apply Sorted_StronglySorted in Hs; [| intros x y z; lia].
  induction Hs as [| a l Hs IH Hall]; cbn; [constructor |].
  destruct (f a); [| exact IH].
  constructor; [exact IH |].
  apply Forall_forall. intros x Hx. apply filter_In in Hx as [Hx _].
  rewrite Forall_forall in Hall. exact (Hall x Hx).
Qed.

Lemma filter_all_false : forall (f : nat -> bool) l,
  (forall u, In u l -> f u = false) -> filter f l = [].
Proof.
  intros f l. induction l as [| h t IH]; intros H; cbn; [reflexivity |].
  rewrite (H h (or_introl eq_refl)). apply IH. intros u Hu. apply H. right. exact Hu.
Qed.

Lemma repeat_sorted : forall (d n : nat), Sorted le (repeat d n).
Proof.
  intros d n. induction n as [| n IH]; cbn; [constructor |].
  constructor; [exact IH | destruct n; cbn; constructor; lia].
Qed.

(* ── 3. The set: classes and the view (F7) ───────────────────────────────── *)

(* The boolean equality of two keys. *)
Definition keq (a b : list nat) : bool := if list_eq_dec Nat.eq_dec a b then true else false.

Lemma keq_true : forall a b, keq a b = true <-> a = b.
Proof.
  intros a b. unfold keq. destruct (list_eq_dec Nat.eq_dec a b) as [-> | Hab]; split;
    intros H; try reflexivity; try discriminate; contradiction.
Qed.

Lemma keq_false : forall a b, keq a b = false <-> a <> b.
Proof.
  intros a b. unfold keq. destruct (list_eq_dec Nat.eq_dec a b) as [-> | Hab]; split;
    intros H; try reflexivity; try discriminate; try contradiction; exact Hab.
Qed.

Lemma existsb_keq : forall x seen, existsb (keq x) seen = true <-> In x seen.
Proof.
  intros x seen. rewrite existsb_exists. split.
  - intros (y & Hy & Hk). apply keq_true in Hk. subst. exact Hy.
  - intros Hin. exists x. split; [exact Hin | apply keq_true; reflexivity].
Qed.

Section Classes.
  (* Sorted payloads, patterns and guards. *)
  Variables V P G : Type.
  (* The sorted payload and the persist flag of the record with the source key k. *)
  Variable val : nat -> V.
  Variable persistent : nat -> bool.
  (* The set key of a sorted payload (set.rs set_key, F5). *)
  Variable vkey : V -> list nat.
  (* F6 (set_key_injective) with an injective bincode: equal keys are equal sorted
     payloads. *)
  Hypothesis vkey_injective : forall u v, vkey u = vkey v -> u = v.
  (* The metered spatial match (SpaceMatcher::bind) and the guard (SpaceMatcher::admits). *)
  Variable spatial : P -> V -> bool.
  Variable admits : G -> V -> bool.
  (* The pattern, the guard, the persist flag and the peek flag of the waiter w. *)
  Variable wpat : nat -> P.
  Variable wguard : nat -> G.
  Variable wpersist : nat -> bool.
  Variable wpeek : nat -> bool.

  (* The class key of the record k (ScanDiscipline::class_key). *)
  Definition rkey (k : nat) : list nat := vkey (val k).

  (* The record s is a member of the class of k. *)
  Definition same (k s : nat) : bool := keq (rkey s) (rkey k).

  Lemma same_true : forall k s, same k s = true <-> rkey s = rkey k.
  Proof. intros k s. apply keq_true. Qed.

  (* Two records of one class have one sorted payload. *)
  Lemma class_val : forall r s, rkey r = rkey s -> val r = val s.
  Proof. intros r s H. apply vkey_injective. exact H. Qed.

  (* The view of a set: the first member of each class, in canonical order. *)
  Fixpoint dedup (seen : list (list nat)) (l : list nat) : list nat :=
    match l with
    | [] => []
    | r :: t =>
        if existsb (keq (rkey r)) seen then dedup seen t else r :: dedup (rkey r :: seen) t
    end.

  Definition view (rs : list nat) : list nat := dedup [] (canon rs).

  (* The first record in canonical order whose sorted payload the pattern binds
     (scan.rs first_bound_record). *)
  Definition first_match (p : P) (rs : list nat) : option nat :=
    find (fun k => spatial p (val k)) (canon rs).

  Lemma dedup_incl : forall seen l v, In v (dedup seen l) -> In v l.
  Proof.
    intros seen l. revert seen. induction l as [| r t IH]; intros seen v Hv; cbn in Hv;
      [destruct Hv |].
    destruct (existsb (keq (rkey r)) seen).
    - right. exact (IH _ _ Hv).
    - destruct Hv as [-> | Hv]; [left; reflexivity | right; exact (IH _ _ Hv)].
  Qed.

  Lemma dedup_fresh : forall seen l v, In v (dedup seen l) -> ~ In (rkey v) seen.
  Proof.
    intros seen l. revert seen. induction l as [| r t IH]; intros seen v Hv; cbn in Hv;
      [destruct Hv |].
    destruct (existsb (keq (rkey r)) seen) eqn:Hr.
    - exact (IH _ _ Hv).
    - destruct Hv as [<- | Hv].
      + intros Hin. apply existsb_keq in Hin. rewrite Hin in Hr. discriminate.
      + intros Hin. apply (IH _ _ Hv). right. exact Hin.
  Qed.

  Lemma dedup_nodup : forall seen l, NoDup (map rkey (dedup seen l)).
  Proof.
    intros seen l. revert seen. induction l as [| r t IH]; intros seen; cbn; [constructor |].
    destruct (existsb (keq (rkey r)) seen); [apply IH |].
    cbn. constructor; [| apply IH].
    intros Hin. apply in_map_iff in Hin as (v & Hv & Hin).
    apply (dedup_fresh _ _ _ Hin). left. symmetry. exact Hv.
  Qed.

  Lemma dedup_covers : forall seen l r,
    In r l -> In (rkey r) seen \/ exists v, In v (dedup seen l) /\ rkey v = rkey r.
  Proof.
    intros seen l. revert seen. induction l as [| x t IH]; intros seen r Hr; [destruct Hr |].
    cbn. destruct (existsb (keq (rkey x)) seen) eqn:Hx.
    - destruct Hr as [<- | Hr].
      + left. apply existsb_keq. exact Hx.
      + exact (IH _ _ Hr).
    - destruct Hr as [<- | Hr].
      + right. exists x. split; [left; reflexivity | reflexivity].
      + destruct (IH (rkey x :: seen) r Hr) as [[Heq | Hin] | (v & Hv & Hk)].
        * right. exists x. split; [left; reflexivity | exact Heq].
        * left. exact Hin.
        * right. exists v. split; [right; exact Hv | exact Hk].
  Qed.

  Lemma dedup_first : forall seen l v,
    In v (dedup seen l) ->
    exists pre post, l = pre ++ v :: post /\ (forall u, In u pre -> rkey u <> rkey v).
  Proof.
    intros seen l. revert seen. induction l as [| r t IH]; intros seen v Hv; cbn in Hv;
      [destruct Hv |].
    destruct (existsb (keq (rkey r)) seen) eqn:Hr.
    - destruct (IH _ _ Hv) as (pre & post & -> & Hpre).
      exists (r :: pre), post. split; [reflexivity |].
      intros u [<- | Hu]; [| exact (Hpre u Hu)].
      intros Heq. apply existsb_keq in Hr.
      apply (dedup_fresh _ _ _ Hv). rewrite <- Heq. exact Hr.
    - destruct Hv as [<- | Hv].
      + exists [], t. split; [reflexivity | intros u []].
      + destruct (IH _ _ Hv) as (pre & post & -> & Hpre).
        exists (r :: pre), post. split; [reflexivity |].
        intros u [<- | Hu]; [| exact (Hpre u Hu)].
        intros Heq. apply (dedup_fresh _ _ _ Hv). left. exact Heq.
  Qed.

  Lemma dedup_app_cons : forall pre seen k post,
    ~ In (rkey k) seen -> (forall u, In u pre -> rkey u <> rkey k) ->
    In k (dedup seen (pre ++ k :: post)).
  Proof.
    induction pre as [| u pre IH]; intros seen k post Hseen Hpre; cbn.
    - destruct (existsb (keq (rkey k)) seen) eqn:Hk.
      + apply existsb_keq in Hk. contradiction.
      + left. reflexivity.
    - destruct (existsb (keq (rkey u)) seen).
      + apply IH; [exact Hseen |]. intros u' Hu'. apply Hpre. right. exact Hu'.
      + right. apply IH.
        * intros [Heq | Hin]; [| contradiction].
          apply (Hpre u (or_introl eq_refl)). exact Heq.
        * intros u' Hu'. apply Hpre. right. exact Hu'.
  Qed.

  (* The first spatial match is the first member of its class in canonical order: every
     member has the sorted payload of the match, so it binds the pattern too. *)
  Lemma first_match_split : forall p rs k,
    first_match p rs = Some k ->
    exists pre post, canon rs = pre ++ k :: post /\
      (forall u, In u pre -> spatial p (val u) = false /\ rkey u <> rkey k) /\
      spatial p (val k) = true.
  Proof.
    intros p rs k Hm. unfold first_match in Hm.
    apply find_some_split in Hm as (pre & post & Hc & Hpre & Hk).
    exists pre, post. split; [exact Hc |]. split; [| exact Hk].
    intros u Hu. split; [exact (Hpre u Hu) |].
    intros Heq. pose proof (Hpre u Hu) as Hu'. rewrite (class_val u k Heq), Hk in Hu'.
    discriminate.
  Qed.

  Lemma first_match_in : forall p rs k, first_match p rs = Some k -> In k rs.
  Proof.
    intros p rs k Hm. destruct (first_match_split p rs k Hm) as (pre & post & Hc & _ & _).
    apply in_canon. rewrite Hc. apply in_or_app. right. left. reflexivity.
  Qed.

  (* F7 set_view_dedups_equal_pars: the view of a set holds one representative for each
     class of the records, the first member of the class in canonical order, and it does not
     depend on the list order of the store. The first record that a pattern binds is a
     representative, so the scan over all records meets the classes in the order of their
     representatives. Two records share a class exactly when their sorted payloads are
     equal. *)
  Theorem set_view_dedups_equal_pars : forall rs,
    (forall r, In r rs -> exists v, In v (view rs) /\ rkey v = rkey r) /\
    NoDup (map rkey (view rs)) /\
    (forall v, In v (view rs) -> In v rs /\
       exists pre post, canon rs = pre ++ v :: post /\
         (forall u, In u pre -> rkey u <> rkey v)) /\
    (forall rs', Permutation rs rs' -> view rs = view rs') /\
    (forall p k, first_match p rs = Some k -> In k (view rs)) /\
    (forall r s, rkey r = rkey s <-> val r = val s).
  Proof.
    intros rs. split; [| split; [| split; [| split; [| split]]]].
    - intros r Hr. unfold view.
      destruct (dedup_covers [] (canon rs) r (proj2 (in_canon r rs) Hr)) as [[] | H].
      exact H.
    - unfold view. apply dedup_nodup.
    - intros v Hv. unfold view in Hv. split.
      + apply in_canon. exact (dedup_incl _ _ _ Hv).
      + exact (dedup_first _ _ _ Hv).
    - intros rs' Hp. unfold view. rewrite (canon_permutation_invariant rs rs' Hp).
      reflexivity.
    - intros p k Hm. destruct (first_match_split p rs k Hm) as (pre & post & Hc & Hpre & _).
      unfold view. rewrite Hc. apply dedup_app_cons; [intros [] |].
      intros u Hu. exact (proj2 (Hpre u Hu)).
    - intros r s. split; [apply class_val | intros H; unfold rkey; rewrite H; reflexivity].
  Qed.

  (* ── 4. The class fire of a receive (F8, F9) ── *)

  (* RECEIVE-SCAN: the first spatial match, and the guard checked once on it (decision 37). A
     refusal parks the receive (None). *)
  Definition receive_choice (p : P) (g : G) (rs : list nat) : option nat :=
    match first_match p rs with
    | Some k => if admits g (val k) then Some k else None
    | None => None
    end.

  Lemma receive_choice_first : forall p g rs k,
    receive_choice p g rs = Some k -> first_match p rs = Some k /\ admits g (val k) = true.
  Proof.
    intros p g rs k H. unfold receive_choice in H.
    destruct (first_match p rs) as [m |]; [| discriminate].
    destruct (admits g (val m)) eqn:Hm; [| discriminate].
    injection H as <-. split; [reflexivity | exact Hm].
  Qed.

  (* The class of the record k in the records rs: every stored member of the class of k, in
     canonical order (scan.rs class_of, F3, decision 41). *)
  Definition class_of (k : nat) (rs : list nat) : list nat := filter (same k) (canon rs).

  (* The records after a consume fire that binds the candidates C: each candidate leaves
     unless it is persistent (a COMM never removes a persistent record). *)
  Fixpoint remove_all (C rs : list nat) : list nat :=
    match C with
    | [] => rs
    | c :: C' => remove_all C' (if persistent c then rs else remove_one c rs)
    end.

  Lemma count_remove_all : forall C rs s,
    count s (remove_all C rs) = if persistent s then count s rs else count s rs - count s C.
  Proof.
    induction C as [| c C IH]; intros rs s; cbn [remove_all count].
    - destruct (persistent s); lia.
    - rewrite IH. destruct (Nat.eqb_spec c s) as [-> | Hcs].
      + destruct (persistent s) eqn:Hp; [reflexivity |].
        pose proof (count_remove_one s rs s) as Hc. rewrite Nat.eqb_refl in Hc.
        cbn [andb] in Hc. destruct (Nat.ltb_spec 0 (count s rs)); lia.
      + destruct (persistent c); [destruct (persistent s); lia |].
        pose proof (count_remove_one c rs s) as Hc.
        destruct (Nat.eqb_spec c s) as [Hcs' | _]; [contradiction |].
        cbn [andb] in Hc. destruct (persistent s); lia.
  Qed.

  Lemma class_of_count : forall k rs s,
    count s (class_of k rs) = if same k s then count s rs else 0.
  Proof.
    intros k rs s. unfold class_of. rewrite count_filter, count_canon. reflexivity.
  Qed.

  Lemma class_of_in : forall k rs s, In s (class_of k rs) -> In s rs /\ rkey s = rkey k.
  Proof.
    intros k rs s H. unfold class_of in H. apply filter_In in H as [Hs Hk].
    split; [apply in_canon; exact Hs | apply same_true; exact Hk].
  Qed.

  (* F8 set_consume_retires_class: a receive that does not peek fires over the whole class
     of its choice, the representative first. Every member binds the pattern and passes the
     guard, the group lists every stored member once, and the fire leaves no non-persistent
     member. Persistent members and the other classes stay. *)
  Theorem set_consume_retires_class : forall p g rs k,
    receive_choice p g rs = Some k ->
    (exists rest, class_of k rs = k :: rest) /\
    (forall s, count s (class_of k rs) = if same k s then count s rs else 0) /\
    (forall s, In s (class_of k rs) -> spatial p (val s) = true /\ admits g (val s) = true) /\
    (forall s, rkey s = rkey k -> persistent s = false ->
       count s (remove_all (class_of k rs) rs) = 0) /\
    (forall s, rkey s <> rkey k \/ persistent s = true ->
       count s (remove_all (class_of k rs) rs) = count s rs).
  Proof.
    intros p g rs k Hk. apply receive_choice_first in Hk as [Hm Hadm].
    destruct (first_match_split p rs k Hm) as (pre & post & Hc & Hpre & Hsp).
    split; [| split; [| split; [| split]]].
    - exists (filter (same k) post). unfold class_of. rewrite Hc, filter_app.
      assert (Hnil : filter (same k) pre = []).
      { apply filter_all_false. intros u Hu.
        destruct (same k u) eqn:Hs; [| reflexivity].
        apply same_true in Hs. exfalso. exact (proj2 (Hpre u Hu) Hs). }
      rewrite Hnil. cbn. unfold same. rewrite (proj2 (keq_true (rkey k) (rkey k)) eq_refl).
      reflexivity.
    - apply class_of_count.
    - intros s Hs. apply class_of_in in Hs as [_ Hs].
      rewrite (class_val s k Hs). split; [exact Hsp | exact Hadm].
    - intros s Hs Hp. rewrite count_remove_all, Hp, class_of_count.
      assert (Hsame : same k s = true) by (apply same_true; exact Hs).
      rewrite Hsame. lia.
    - intros s [Hs | Hp]; rewrite count_remove_all.
      + destruct (persistent s); [reflexivity |].
        rewrite class_of_count.
        assert (Hsame : same k s = false) by (apply keq_false; exact Hs).
        rewrite Hsame. lia.
      + rewrite Hp. reflexivity.
  Qed.

  (* A staged consume fire: the channel group, the patterns, the candidates (each with its
     channel) and the number of results that the outcome dispatches (generic.rs
     fire_receive_group, retire, outcome). *)
  Record staged := {
    chans : list nat;
    pats : list nat;
    cands : list (nat * nat);
    dispatched : nat
  }.

  (* The consume over [D; n] of the class of k (scan.rs fire_class): one channel, one
     pattern and one candidate for each member, and the result of the representative only. *)
  Definition class_fire (d p : nat) (k : nat) (rs : list nat) : staged :=
    let C := class_of k rs in
    {| chans := repeat d (length C); pats := repeat p (length C);
       cands := map (fun s => (d, s)) C; dispatched := 1 |}.

  (* The consume of K_retire(m) over [D] (scan.rs retire_class): never dispatched. *)
  Definition retire_fire (d rp m : nat) : staged :=
    {| chans := [d]; pats := [rp]; cands := [(d, m)]; dispatched := 0 |}.

  (* The staging rules of a consume fire in the substrate (space_store_handle.rs): one
     pattern for each channel (check_arity), a canonical group, non-decreasing by channel
     hash and possibly repeating a channel (canonical_group_key, FR-1'), candidates that line
     up with the channels (check_lined_up), at least one candidate, stored candidates only
     (check_stored_candidates), distinct claims for the non-persistent candidates
     (claim_positions), and at most one dispatched result for each candidate (outcome). *)
  Definition stages (rs : list nat) (s : staged) : Prop :=
    length (pats s) = length (chans s) /\
    Sorted le (chans s) /\
    map fst (cands s) = chans s /\
    1 <= length (cands s) /\
    (forall c, In c (cands s) -> In (snd c) rs) /\
    (forall x, persistent x = false -> count x (map snd (cands s)) <= count x rs) /\
    dispatched s <= length (cands s).

  Lemma map_snd_pairs : forall (d : nat) (l : list nat), map snd (map (fun s => (d, s)) l) = l.
  Proof. intros d l. rewrite map_map. apply map_id. Qed.

  Lemma map_fst_pairs : forall (d : nat) (l : list nat),
    map fst (map (fun s => (d, s)) l) = repeat d (length l).
  Proof. intros d l. induction l as [| x l IH]; cbn; [reflexivity | rewrite IH; reflexivity]. Qed.

  (* F9 set_retire_comm_lines_up: the consume over [D; n] of a class and every K_retire
     consume meet the staging rules of the substrate. The class fire dispatches one result,
     and a retire dispatches none. *)
  Theorem set_retire_comm_lines_up : forall d p rp k m rs,
    In k rs ->
    (stages rs (class_fire d p k rs) /\ dispatched (class_fire d p k rs) = 1) /\
    (In m rs -> stages rs (retire_fire d rp m) /\ dispatched (retire_fire d rp m) = 0).
  Proof.
    intros d p rp k m rs Hk. split.
    - assert (Hin : In k (class_of k rs)).
      { unfold class_of. apply filter_In. split; [apply in_canon; exact Hk |].
        apply same_true. reflexivity. }
      split; [| reflexivity]. unfold stages, class_fire; cbn [chans pats cands dispatched].
      rewrite map_snd_pairs, map_fst_pairs, !repeat_length, length_map.
      split; [reflexivity |]. split; [apply repeat_sorted |]. split; [reflexivity |].
      split; [destruct (class_of k rs) as [| x l]; [destruct Hin | cbn; lia] |].
      split; [| split].
      + intros c Hc. apply in_map_iff in Hc as (s & <- & Hs). cbn [snd].
        exact (proj1 (class_of_in k rs s Hs)).
      + intros x _. rewrite class_of_count. destruct (same k x); lia.
      + destruct (class_of k rs) as [| x l]; [destruct Hin | cbn; lia].
    - intros Hm. split; [| reflexivity]. unfold stages, retire_fire; cbn.
      split; [reflexivity |]. split; [repeat constructor |]. split; [reflexivity |].
      split; [lia |]. split; [| split].
      + intros c [<- | []]. exact Hm.
      + intros x _. rewrite Nat.add_0_r. destruct (Nat.eqb_spec m x) as [-> | _];
          [apply count_in; exact Hm | lia].
      + lia.
  Qed.

  (* ── 5. OFFER-SCAN with RETIRE-CLASS (F10) and RESTORE-SCAN (Rule E) ── *)

  (* A staged operation of the scan driver of a set. *)
  Inductive op :=
  | Store (k : nat)                   (* Cx::store_on of the record k *)
  | Park (c : nat)                    (* Cx::park of the continuation c *)
  | FireIn (w : nat) (k : nat)        (* Cx::fire_offer_on: the element k fires the waiter w *)
  | FireGroup (c : nat) (C : list nat)  (* Cx::fire_receive_group: c takes the records C *)
  | Retire (m : nat).                 (* Cx::retire: K_retire of the stored member m *)

  (* The records of D after the operation [o] on the records [rs]. A store puts its record
     at the front, as put_datum does. The incoming element of a produce-side fire is never
     stored. A consume fire, a peek fire included, removes each non-persistent record that
     it binds (decision 45, Rule E). *)
  Definition on_records (o : op) (rs : list nat) : list nat :=
    match o with
    | Store k => k :: rs
    | Park _ => rs
    | FireIn _ _ => rs
    | FireGroup _ C => remove_all C rs
    | Retire m => if persistent m then rs else remove_one m rs
    end.

  Definition apply_ops (ops : list op) (rs : list nat) : list nat :=
    fold_left (fun acc o => on_records o acc) ops rs.

  Lemma apply_ops_app : forall l1 l2 rs, apply_ops (l1 ++ l2) rs = apply_ops l2 (apply_ops l1 rs).
  Proof. intros l1 l2 rs. unfold apply_ops. apply fold_left_app. Qed.

  (* RETIRE-CLASS of the class of the element x (scan.rs retire_class): K_retire of each
     stored non-persistent member, in canonical order. *)
  Definition retire_class (x : nat) (rs : list nat) : list op :=
    map Retire (filter (fun s => same x s && negb (persistent s)) (canon rs)).

  Lemma apply_retires : forall M rs, apply_ops (map Retire M) rs = remove_all M rs.
  Proof.
    induction M as [| m M IH]; intros rs; cbn; [reflexivity |].
    unfold apply_ops in IH |- *. cbn [fold_left]. apply IH.
  Qed.

  (* The members of the class of x that a set may not keep after a fire of x that does not
     peek: the stored non-persistent members. *)
  Definition class_cleared (x : nat) (rs : list nat) : Prop :=
    forall s, same x s = true -> persistent s = false -> count s rs = 0.

  Lemma retire_class_clears : forall x rs,
    class_cleared x (apply_ops (retire_class x rs) rs) /\
    (forall s, same x s = false \/ persistent s = true ->
       count s (apply_ops (retire_class x rs) rs) = count s rs).
  Proof.
    intros x rs. unfold retire_class. rewrite apply_retires. split.
    - intros s Hs Hp. rewrite count_remove_all, Hp, count_filter, count_canon, Hs, Hp.
      cbn. lia.
    - intros s [Hs | Hp]; rewrite count_remove_all.
      + destruct (persistent s); [reflexivity |].
        rewrite count_filter, Hs. cbn. lia.
      + rewrite Hp. reflexivity.
  Qed.

  Lemma retires_keep_cleared : forall x rs,
    class_cleared x rs -> class_cleared x (apply_ops (retire_class x rs) rs).
  Proof. intros x rs _. exact (proj1 (retire_class_clears x rs)). Qed.

  (* The waiter w takes the element x: its pattern binds the sorted payload of x and its
     guard admits it. *)
  Definition fits (x w : nat) : bool := spatial (wpat w) (val x) && admits (wguard w) (val x).

  (* The end of a plan: the element settled (stored, or spent by a fire), or it is still in
     flight and its operation re-issues (A10, I-21). *)
  Inductive ending := Settled | Reissue.

  (* OFFER-SCAN of the element x over the waiters [wl] in canonical order, with the
     records rs (scan.rs offer_scan). A waiter that does not fit is skipped, and a guard
     refusal moves to the next waiter (decision 37). A fire of a waiter that does not peek
     retires the stored members of the class of x (RETIRE-CLASS). A fire that leaves x in
     flight continues with the next waiter (decision 30). x is spent when it is not
     persistent and the waiter does not peek, and the waiter stays when it is persistent. *)
  Fixpoint offer_pass (wl : list nat) (x : nat) (rs : list nat) (rounds : nat)
    : list op * ending :=
    match wl with
    | [] => ([Store x], Settled)
    | w :: rest =>
        if fits x w then
          let retires := if wpeek w then [] else retire_class x rs in
          if negb (persistent x) && negb (wpeek w) then (FireIn w x :: retires, Settled)
          else if wpersist w then (FireIn w x :: retires, Reissue)
          else match rounds with
               | 0 => (FireIn w x :: retires, Reissue)
               | S r =>
                   let (ops, e) := offer_pass rest x (apply_ops retires rs) r in
                   (FireIn w x :: retires ++ ops, e)
               end
        else offer_pass rest x rs rounds
    end.

  Lemma not_in_retires_fire : forall x rs w k, ~ In (FireIn w k) (retire_class x rs).
  Proof.
    intros x rs w k Hin. unfold retire_class in Hin. apply in_map_iff in Hin as (m & H & _).
    discriminate.
  Qed.

  Lemma in_retires_retire : forall x rs m,
    In (Retire m) (retire_class x rs) -> In m rs /\ same x m = true /\ persistent m = false.
  Proof.
    intros x rs m Hin. unfold retire_class in Hin. apply in_map_iff in Hin as (m' & H & Hm).
    injection H as ->. apply filter_In in Hm as [Hm Hf].
    apply andb_prop in Hf as [Hs Hp]. apply negb_true_iff in Hp.
    split; [apply in_canon; exact Hm | split; assumption].
  Qed.

  (* Every K_retire of an offer follows a fire of a waiter that does not peek. *)
  Lemma offer_retire_after_fire : forall wl x rs rounds m,
    In (Retire m) (fst (offer_pass wl x rs rounds)) ->
    exists w, In (FireIn w x) (fst (offer_pass wl x rs rounds)) /\ wpeek w = false.
  Proof.
    induction wl as [| w rest IH]; intros x rs rounds m Hin; cbn [offer_pass] in Hin |- *.
    - destruct Hin as [H | []]. discriminate.
    - destruct (fits x w); [| exact (IH _ _ _ _ Hin)].
      destruct (wpeek w) eqn:Hpk.
      + destruct (negb (persistent x) && negb true); [destruct Hin as [H | []]; discriminate |].
        destruct (wpersist w); [destruct Hin as [H | []]; discriminate |].
        destruct rounds as [| r]; [destruct Hin as [H | []]; discriminate |].
        destruct (offer_pass rest x (apply_ops [] rs) r) as [ops e] eqn:Hrest.
        cbn [fst] in Hin |- *. destruct Hin as [H | Hin]; [discriminate |].
        cbn [app] in Hin.
        assert (Hops : In (Retire m) (fst (offer_pass rest x (apply_ops [] rs) r)))
          by (rewrite Hrest; exact Hin).
        destruct (IH _ _ _ _ Hops) as (w' & Hw' & Hpk').
        exists w'. split; [| exact Hpk']. right. rewrite Hrest in Hw'. exact Hw'.
      + exists w. split; [| exact Hpk].
        destruct (negb (persistent x) && negb false); [left; reflexivity |].
        destruct (wpersist w); [left; reflexivity |].
        destruct rounds as [| r]; [left; reflexivity |].
        destruct (offer_pass rest x (apply_ops (retire_class x rs) rs) r).
        left. reflexivity.
  Qed.

  (* A fire of an offer whose waiters all peek fires one of those waiters. *)
  Lemma offer_fired_waiter_in : forall x wl rs rounds w,
    (forall u, In u wl -> wpeek u = true) ->
    In (FireIn w x) (fst (offer_pass wl x rs rounds)) -> In w wl.
  Proof.
    intros x. induction wl as [| u wl IH]; intros rs rounds w Hpeeks Hf;
      cbn [offer_pass] in Hf.
    - destruct Hf as [H | []]. discriminate.
    - destruct (fits x u).
      + rewrite (Hpeeks u (or_introl eq_refl)) in Hf. cbn [negb] in Hf.
        rewrite andb_false_r in Hf.
        destruct (wpersist u).
        * destruct Hf as [H | []]. injection H as ->. left. reflexivity.
        * destruct rounds as [| r'].
          -- destruct Hf as [H | []]. injection H as ->. left. reflexivity.
          -- destruct (offer_pass wl x (apply_ops [] rs) r') as [ops e] eqn:Hrest.
             cbn [fst app] in Hf. destruct Hf as [H | Hf].
             ++ injection H as ->. left. reflexivity.
             ++ right. apply (IH (apply_ops [] rs) r').
                ** intros u' Hu'. apply Hpeeks. right. exact Hu'.
                ** rewrite Hrest. exact Hf.
      + right. apply (IH rs rounds); [intros u' Hu'; apply Hpeeks; right; exact Hu' | exact Hf].
  Qed.

  (* An offer whose waiters all peek stages no K_retire. *)
  Lemma offer_peeks_retire_nothing : forall wl x rs rounds,
    (forall w, In w wl -> wpeek w = true) ->
    forall m, ~ In (Retire m) (fst (offer_pass wl x rs rounds)).
  Proof.
    intros wl x rs rounds Hall m Hin.
    destruct (offer_retire_after_fire wl x rs rounds m Hin) as (w & Hw & Hpk).
    pose proof (Hall w (offer_fired_waiter_in x wl rs rounds w Hall Hw)) as Hpw.
    rewrite Hpk in Hpw. discriminate.
  Qed.

  (* An offer of a persistent element keeps a cleared class cleared: its fires change no
     record, its retires remove members, and its store adds a persistent record. *)
  Lemma offer_keeps_cleared : forall wl x rs rounds,
    persistent x = true -> class_cleared x rs ->
    class_cleared x (apply_ops (fst (offer_pass wl x rs rounds)) rs).
  Proof.
    induction wl as [| w rest IH]; intros x rs rounds Hx Hcl; cbn [offer_pass].
    - intros s Hs Hp. unfold apply_ops; cbn [fst fold_left on_records count].
      destruct (Nat.eqb_spec x s) as [-> | _]; [rewrite Hx in Hp; discriminate |].
      exact (Hcl s Hs Hp).
    - destruct (fits x w); [| apply IH; assumption].
      assert (Hret : class_cleared x
                (apply_ops (if wpeek w then [] else retire_class x rs) rs)).
      { destruct (wpeek w); [exact Hcl | apply retires_keep_cleared; exact Hcl]. }
      rewrite Hx. cbn [negb andb].
      destruct (wpersist w); [cbn [fst]; exact Hret |].
      destruct rounds as [| r]; [cbn [fst]; exact Hret |].
      destruct (offer_pass rest x (apply_ops (if wpeek w then [] else retire_class x rs) rs) r)
        as [ops e] eqn:Hrest.
      cbn [fst]. change (FireIn w x :: (if wpeek w then [] else retire_class x rs) ++ ops)
        with ([FireIn w x] ++ (if wpeek w then [] else retire_class x rs) ++ ops).
      rewrite !apply_ops_app. unfold apply_ops at 3. cbn [fold_left on_records].
      replace ops with (fst (offer_pass rest x
        (apply_ops (if wpeek w then [] else retire_class x rs) rs) r)) by (rewrite Hrest; reflexivity).
      apply IH; assumption.
  Qed.

  (* After a fire of a waiter that does not peek, the plan ends with the class of x cleared:
     the fire spends x, or x is persistent and the rest of the plan keeps the class
     cleared. *)
  Lemma offer_fire_clears : forall wl x rs rounds w,
    In (FireIn w x) (fst (offer_pass wl x rs rounds)) -> wpeek w = false ->
    class_cleared x (apply_ops (fst (offer_pass wl x rs rounds)) rs).
  Proof.
    induction wl as [| w0 rest IH]; intros x rs rounds w Hin Hpk; cbn [offer_pass] in Hin |- *.
    - destruct Hin as [H | []]. discriminate.
    - destruct (fits x w0); [| exact (IH _ _ _ _ Hin Hpk)].
      set (retires := if wpeek w0 then [] else retire_class x rs) in *.
      assert (Hone : class_cleared x (apply_ops retires rs) ->
                class_cleared x (apply_ops (FireIn w0 x :: retires) rs)).
      { intros Hcl. change (FireIn w0 x :: retires) with ([FireIn w0 x] ++ retires).
        rewrite apply_ops_app. exact Hcl. }
      assert (Hfirst : In (FireIn w x) (FireIn w0 x :: retires) -> w = w0).
      { intros [H | H]; [injection H as ->; reflexivity |].
        exfalso. unfold retires in H. destruct (wpeek w0); [destruct H |].
        exact (not_in_retires_fire x rs w x H). }
      assert (Hclear : w = w0 -> class_cleared x (apply_ops retires rs)).
      { intros ->. unfold retires. rewrite Hpk. exact (proj1 (retire_class_clears x rs)). }
      destruct (negb (persistent x) && negb (wpeek w0)) eqn:Hdone.
      + cbn [fst] in Hin |- *. apply Hone. apply Hclear. exact (Hfirst Hin).
      + destruct (wpersist w0).
        * cbn [fst] in Hin |- *. apply Hone. apply Hclear. exact (Hfirst Hin).
        * destruct rounds as [| r].
          -- cbn [fst] in Hin |- *. apply Hone. apply Hclear. exact (Hfirst Hin).
          -- destruct (offer_pass rest x (apply_ops retires rs) r) as [ops e] eqn:Hrest.
             cbn [fst] in Hin |- *.
             assert (Hops : ops = fst (offer_pass rest x (apply_ops retires rs) r))
               by (rewrite Hrest; reflexivity).
             assert (Hshape : apply_ops (FireIn w0 x :: retires ++ ops) rs
                              = apply_ops ops (apply_ops retires rs)).
             { change (FireIn w0 x :: retires ++ ops) with ([FireIn w0 x] ++ retires ++ ops).
               rewrite !apply_ops_app. reflexivity. }
             rewrite Hshape. destruct Hin as [Hin | Hin].
             ++ (* The fire of w0 itself: x is persistent here, so the rest keeps the
                   class cleared. *)
                injection Hin as Hw. subst w0. rewrite Hpk in Hdone. cbn [negb] in Hdone.
                rewrite andb_true_r in Hdone. apply negb_false_iff in Hdone.
                rewrite Hops. apply offer_keeps_cleared; [exact Hdone |].
                apply Hclear. reflexivity.
             ++ apply in_app_or in Hin as [Hin | Hin].
                ** exfalso. unfold retires in Hin. destruct (wpeek w0); [destruct Hin |].
                   exact (not_in_retires_fire x rs w x Hin).
                ** rewrite Hops in Hin |- *. exact (IH _ _ _ _ Hin Hpk).
  Qed.

  (* F10 set_produce_fire_retires_stored_class: RETIRE-CLASS stages K_retire of each stored
     non-persistent member of the class of the element, and it changes no other record. In an
     offer, a fire of a waiter that does not peek leaves no non-persistent stored member of
     the class when the plan ends, and an offer whose fired waiters all peek retires
     nothing. *)
  Theorem set_produce_fire_retires_stored_class : forall x rs,
    class_cleared x (apply_ops (retire_class x rs) rs) /\
    (forall s, same x s = false \/ persistent s = true ->
       count s (apply_ops (retire_class x rs) rs) = count s rs) /\
    (forall m, In (Retire m) (retire_class x rs) ->
       In m rs /\ same x m = true /\ persistent m = false) /\
    (forall ws rounds w, In (FireIn w x) (fst (offer_pass ws x rs rounds)) -> wpeek w = false ->
       class_cleared x (apply_ops (fst (offer_pass ws x rs rounds)) rs)) /\
    (forall ws rounds, (forall w, In w ws -> wpeek w = true) ->
       forall m, ~ In (Retire m) (fst (offer_pass ws x rs rounds))).
  Proof.
    intros x rs. split; [| split; [| split; [| split]]].
    - exact (proj1 (retire_class_clears x rs)).
    - exact (proj2 (retire_class_clears x rs)).
    - apply in_retires_retire.
    - intros ws rounds w. apply offer_fire_clears.
    - intros ws rounds. apply offer_peeks_retire_nothing.
  Qed.

  (* ── 6. A peek binds the representative alone, and RESTORE-SCAN sends it again ── *)

  (* The group of a peek: the representative alone on [D] (scan.rs class_of for a peek). *)
  Definition peek_group (k : nat) (rs : list nat) : list nat := [k].

  (* RECEIVE-SCAN of a peek of the continuation c with the pattern p and the guard g, over
     the records rs and the waiters ws: the fire of the representative, which removes it when
     it is not persistent (Rule E), then RESTORE-SCAN, the offer of the removed record to the
     waiters with the rounds of the receive (scan.rs receive, restore_in, restore_scan). The
     second send is not persistent, so the model asks its record to be one that is not
     persistent, as the receive only restores such a record. *)
  Definition peek_plan (c : nat) (p : P) (g : G) (rs ws : list nat) (rounds : nat)
    : list op * ending :=
    match receive_choice p g rs with
    | None => ([Park c], Settled)
    | Some k =>
        if persistent k then ([FireGroup c (peek_group k rs)], Settled)
        else
          let (ops, e) := offer_pass (canon ws) k (remove_all (peek_group k rs) rs) rounds in
          (FireGroup c (peek_group k rs) :: ops, e)
    end.

  Lemma offer_none_stores : forall wl x rs rounds,
    (forall w, In w wl -> fits x w = false) -> offer_pass wl x rs rounds = ([Store x], Settled).
  Proof.
    induction wl as [| w wl IH]; intros x rs rounds H; cbn; [reflexivity |].
    rewrite (H w (or_introl eq_refl)). apply IH. intros u Hu. apply H. right. exact Hu.
  Qed.

  (* set_peek_restores_representative_only (decision 45, Rule E): a peek binds the
     representative of its choice alone. A persistent representative stays and nothing is
     sent again. Otherwise the fire removes the representative and RESTORE-SCAN sends it
     again: when no waiter takes it, it is stored again and the records are as before. The
     peek retires nothing itself: a K_retire of the plan follows a fire of the second send on
     a waiter that does not peek, and then the class of the representative leaves the set. *)
  Theorem set_peek_restores_representative_only : forall c p g rs ws rounds k,
    receive_choice p g rs = Some k ->
    In k (view rs) /\
    (exists rest, fst (peek_plan c p g rs ws rounds) = FireGroup c [k] :: rest) /\
    (persistent k = true ->
       peek_plan c p g rs ws rounds = ([FireGroup c [k]], Settled) /\
       apply_ops (fst (peek_plan c p g rs ws rounds)) rs = rs) /\
    (persistent k = false -> (forall w, In w ws -> fits k w = false) ->
       fst (peek_plan c p g rs ws rounds) = [FireGroup c [k]; Store k] /\
       Permutation (apply_ops (fst (peek_plan c p g rs ws rounds)) rs) rs) /\
    (forall m, In (Retire m) (fst (peek_plan c p g rs ws rounds)) ->
       exists w, In (FireIn w k) (fst (peek_plan c p g rs ws rounds)) /\ wpeek w = false /\
         class_cleared k (apply_ops (fst (peek_plan c p g rs ws rounds)) rs)).
  Proof.
    intros c p g rs ws rounds k Hk.
    pose proof (receive_choice_first p g rs k Hk) as [Hm _].
    assert (Hin : In k rs) by exact (first_match_in p rs k Hm).
    destruct (set_view_dedups_equal_pars rs) as (_ & _ & _ & _ & Hrep & _).
    split; [exact (Hrep p k Hm) |].
    unfold peek_plan. rewrite Hk. unfold peek_group.
    destruct (persistent k) eqn:Hp.
    - split; [exists []; reflexivity |]. split; [| split].
      + intros _. split; [reflexivity |]. unfold apply_ops. cbn [fst fold_left on_records].
        cbn [remove_all]. rewrite Hp. reflexivity.
      + intros H. discriminate.
      + intros m [H | []]. discriminate.
    - destruct (offer_pass (canon ws) k (remove_all [k] rs) rounds) as [ops e] eqn:Hoffer.
      cbn [fst]. split; [exists ops; reflexivity |]. split; [| split].
      + intros H. discriminate.
      + intros _ Hnone.
        rewrite offer_none_stores in Hoffer.
        * injection Hoffer as <- _. split; [reflexivity |].
          unfold apply_ops. cbn [fold_left on_records remove_all]. rewrite Hp.
          apply Permutation_sym. exact (remove_one_cons_perm k rs Hin).
        * intros w Hw. apply Hnone. apply in_canon. exact Hw.
      + intros m [H | Hm']; [discriminate |].
        assert (Hops : ops = fst (offer_pass (canon ws) k (remove_all [k] rs) rounds))
          by (rewrite Hoffer; reflexivity).
        rewrite Hops in Hm'.
        destruct (offer_retire_after_fire _ _ _ _ _ Hm') as (w & Hw & Hpk).
        exists w. split; [right; rewrite Hops; exact Hw |]. split; [exact Hpk |].
        assert (Hshape : apply_ops (FireGroup c [k] :: ops) rs = apply_ops ops (remove_all [k] rs))
          by reflexivity.
        rewrite Hshape, Hops. exact (offer_fire_clears _ _ _ _ _ Hw Hpk).
  Qed.
End Classes.

(* ── 7. Membership: the hashes first, the full keys only on equal hashes (DR-90) ── *)

Section Membership.
  (* The metered hash of a key (space_store_handle.rs metered_hash: blake2b-256 of the
     bincode bytes of the key). The model takes any function: no property of the hash is
     assumed. *)
  Variable h : list nat -> list nat.

  (* A comparison that the membership test reserves and makes. *)
  Inductive comparison := CompareHashes | CompareKeys.

  (* scan.rs same_class: the key [key], whose hash is [key_hash], against the key [other].
     The hashes are compared first, and the full keys only when the hashes are equal. The
     result lists the comparisons and gives the answer. *)
  Definition same_class (key key_hash other : list nat) : list comparison * bool :=
    if list_eq_dec Nat.eq_dec key_hash (h other)
    then ([CompareHashes; CompareKeys], keq key other)
    else ([CompareHashes], false).

  (* set_membership_full_compare_on_equal_hash (DR-90): the membership test answers key
     equality exactly, for every hash function: a hash collision never makes two classes
     one. It compares the full keys exactly when the hashes are equal. *)
  Theorem set_membership_full_compare_on_equal_hash : forall key other,
    (snd (same_class key (h key) other) = true <-> key = other) /\
    (In CompareKeys (fst (same_class key (h key) other)) <-> h key = h other) /\
    (h key = h other -> key <> other -> snd (same_class key (h key) other) = false).
  Proof.
    intros key other. unfold same_class.
    destruct (list_eq_dec Nat.eq_dec (h key) (h other)) as [Heq | Hne]; cbn [fst snd].
    - split; [apply keq_true |]. split.
      + split; [intros _; exact Heq | intros _; right; left; reflexivity].
      + intros _ Hkey. apply keq_false. exact Hkey.
    - split.
      + split; [discriminate |]. intros ->. exfalso. apply Hne. reflexivity.
      + split.
        * split; [intros [H | []]; discriminate | intros H; contradiction].
        * intros H. contradiction.
  Qed.
End Membership.

(* Disabled by decision 96 (decision 89, DR-131): set_key_trace_covered goes
   with the host-work metering. The section stays below as a comment.
(* ── 8. Metering of the set key and of the membership test (decision 42) ── *)

(* The metering rule of DR-88, restated from AgentReadCoverage.v and DataCollection.v: a
   trace of reservations and reads, covered when every prefix reads at most what it
   reserved. The model counts one host-work dimension. Each dimension satisfies the rule on
   its own. *)
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

Lemma covered_nil : covered [].
Proof. intros prefix suffix Hs. destruct prefix; [cbn; lia | discriminate]. Qed.

Lemma steps_covered : forall units,
  covered (flat_map step units) /\
  total read_units (flat_map step units) <= total reserved_units (flat_map step units).
Proof.
  induction units as [| u rest [IHc IHb]]; cbn [flat_map].
  - split; [exact covered_nil | cbn; lia].
  - split.
    + apply covered_app; [apply step_covered | apply step_balanced | exact IHc].
    + rewrite !total_app. pose proof (step_balanced u). lia.
Qed.

(* The work of one set key and of one membership test, in units: the buffer of the element
   encodings, the sort and the encode of each element, the key buffer, the hash of the other
   key, and the comparisons. *)
Record key_work := {
  elements_buffer : nat;
  element_work : list (nat * nat);
  key_buffer : nat;
  other_hash : nat;
  comparisons : list nat
}.

(* set.rs set_key: meter.buffer reserves the buffer of the encodings before it allocates
   it; the metered sorter and the metered writer reserve each read and write of an element
   before they make it; meter.buffer reserves the key buffer before it is allocated and
   filled. scan.rs same_class: metered_hash reserves each write, and each comparison is
   reserved before it is made. *)
Definition key_trace (kw : key_work) : list event :=
  step (elements_buffer kw)
  ++ flat_map (fun se => step (fst se) ++ step (snd se)) (element_work kw)
  ++ step (key_buffer kw)
  ++ step (other_hash kw)
  ++ flat_map step (comparisons kw).

(* set_key_trace_covered (decision 42): every prefix of the trace of a set key and of a
   membership test reads at most what it reserved. So a budget below the reservations of an
   operation is exceeded by a reservation before the commit, and the operation has no
   effect. *)
Theorem set_key_trace_covered : forall kw, covered (key_trace kw).
Proof.
  intros kw. unfold key_trace.
  assert (Helements :
    covered (flat_map (fun se => step (fst se) ++ step (snd se)) (element_work kw)) /\
    total read_units (flat_map (fun se => step (fst se) ++ step (snd se)) (element_work kw))
    <= total reserved_units
         (flat_map (fun se => step (fst se) ++ step (snd se)) (element_work kw))).
  { induction (element_work kw) as [| [s e] rest [IHc IHb]]; cbn [flat_map fst snd].
    - split; [exact covered_nil | cbn; lia].
    - rewrite <- app_assoc. split.
      + apply covered_app; [apply step_covered | apply step_balanced |].
        apply covered_app; [apply step_covered | apply step_balanced | exact IHc].
      + rewrite !total_app. pose proof (step_balanced s). pose proof (step_balanced e). lia. }
  destruct Helements as [Hec Heb].
  destruct (steps_covered (comparisons kw)) as [Hcc Hcb].
  apply covered_app; [apply step_covered | apply step_balanced |].
  apply covered_app; [exact Hec | exact Heb |].
  apply covered_app; [apply step_covered | apply step_balanced |].
  apply covered_app; [apply step_covered | apply step_balanced |].
  exact Hcc.
Qed.
*)

Print Assumptions set_key_injective.
Print Assumptions set_view_dedups_equal_pars.
Print Assumptions set_consume_retires_class.
Print Assumptions set_retire_comm_lines_up.
Print Assumptions set_produce_fire_retires_stored_class.
Print Assumptions set_peek_restores_representative_only.
Print Assumptions set_membership_full_compare_on_equal_hash.
(* Disabled by decision 96 (decision 89, DR-131): set_key_trace_covered goes
   with the host-work metering.
Print Assumptions set_key_trace_covered.
*)
