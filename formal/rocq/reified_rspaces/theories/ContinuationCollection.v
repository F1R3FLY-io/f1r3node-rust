(* ═══════════════════════════════════════════════════════════════════════════
   ContinuationCollection.v — the waiter side of the collections of P5
   (P5a: decisions 37 and 41, D10, F14)

   The waiting continuations of a collection live on its data channel
   D(sid,k). A bag and a set visit every waiter in the canonical order of
   DR-75 (decision 41): a send fires the first waiter whose pattern binds the
   datum and whose guard admits it, and a guard refusal moves the send to the
   next waiter (decision 37, the produce side). A head-only discipline (queue,
   stack, priority queue) and a cell offer the datum to one waiter only: the
   head of the side. A fire removes exactly the waiter that it fires, by its
   record identity (D10). A park whose bytes equal a parked waiter changes
   nothing, and any byte difference parks another waiter (F14).

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md, A.7 (OFFER-SCAN) and
     A.13 (this module: F12 to F16). The P5 design v2, 2.8.1 (F12 to F16)
     and 2.4 (the legacy ContinuationCollection, removal by pattern
     equality).
   - Plan: decisions 11 (head-only), 37 (the current guard rule) and 41
     (source-hash order).
   - Code: rholang/src/rust/interpreter/spaces/disciplines/scan.rs of the BAG
     batch of P5 wave W2, and the substrate rspace++/src/rspace/
     space_store_handle.rs. The line numbers below are the line numbers of
     that batch.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   visit, canon, insert_key space_store_handle.rs
                            canonical_continuation_order (DR-75): the
                            waiters by the hash of their source consume
   fire_remove, remove_one  space_store_handle.rs remove_continuation and
                            first_equal_continuation: the first waiter
                            whose bytes equal the fired waiter, so a
                            removal by record identity (D10)
   remove_by_pattern        the legacy rule (legacy generic_rspace.rs,
                            continuation removal by pattern equality)
   park                     space_store_handle.rs put_continuation: no
                            insert when a waiter with the same identity is
                            parked (F14)
   fits, offer_choice       scan.rs:533-565 (first_admitting_waiter: the
                            pattern copy and the metered match of each
                            tested waiter, the guard of a waiter that
                            binds, and the next waiter on a refusal)
   candidates               scan.rs:408-499 (Scan: every waiter in
                            canonical order) and generic.rs offer_head
                            with Cx::head_waiter (Head: the head waiter
                            only, decision 11)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A waiter is its key: the hash of its source consume, which covers its
     channels, patterns, continuation bytes and persist flag. Equal keys are
     byte-identical waiters. The pattern and the guard of a waiter are
     functions of its key.
   - spatial is the metered match of a pattern against a payload and admits
     is the guard. same_pattern is the pattern equality of the legacy rule.
     The model takes them as section variables: every theorem holds for every
     matcher.
   - The head of a head-only side is its least entry by the order of the
     discipline. The model writes that order as the key order, so the head is
     the first entry of the sorted side.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. waiter_removal_by_record_identity F12: a fire removes exactly the
                                         fired waiter and keeps every other
                                         waiter
    2. pattern_equality_removal_can_remove_another_waiter
                                         F13: the legacy removal by pattern
                                         equality removes another waiter
                                         with an equal pattern and keeps
                                         the fired one
    3. park_byte_identity_idempotent     F14: an equal park changes nothing,
                                         and a new waiter is added once
    4. waiter_visit_order_canonical      F15: the waiter order is sorted and
                                         permutation-invariant, and a send
                                         fires the first waiter in that order
                                         that fits (a refusal moves on)
    5. head_waiter_unique                F16: a head-only side offers at most
                                         one candidate, the least one, where
                                         a scan side offers every waiter

   The negative controls of scripts/verify-reified/rocq-negative-controls.py
   (fragment of the BAG batch) mutate one rule each: the removal by pattern
   equality, a legacy rule that removes by identity, a park that always adds,
   the order by list position, a produce side that stops at the first
   spatial match, and a head-only side with two candidates.

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

(* The canonical order of the waiters [l]: their keys in ascending order. *)
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

(* The order in which the driver visits the waiters of a collection: the
   canonical order (Cx::canonical_waiter_order). *)
Definition visit (l : list nat) : list nat := canon l.

(* ── 2. Lists of waiters ─────────────────────────────────────────────────── *)

(* The removal of the first waiter with the key [w]: a removal by record
   identity. *)
Fixpoint remove_one (w : nat) (l : list nat) : list nat :=
  match l with
  | [] => []
  | h :: t => if h =? w then t else h :: remove_one w t
  end.

Lemma remove_one_cons_perm : forall w l, In w l -> Permutation l (w :: remove_one w l).
Proof.
  intros w l. induction l as [| h t IH]; cbn; intros Hin; [destruct Hin |].
  destruct (Nat.eqb_spec h w) as [-> | Hhw]; [reflexivity |].
  destruct Hin as [Hin | Hin]; [contradiction |].
  eapply perm_trans; [apply perm_skip; exact (IH Hin) | apply perm_swap].
Qed.

Lemma remove_one_incl : forall w l x, In x (remove_one w l) -> In x l.
Proof.
  intros w l x. induction l as [| h t IH]; cbn; intros Hin; [exact Hin |].
  destruct (h =? w); [right; exact Hin |].
  destruct Hin as [-> | Hin]; [left; reflexivity | right; apply IH; exact Hin].
Qed.

Lemma remove_one_keeps : forall w l x, x <> w -> In x l -> In x (remove_one w l).
Proof.
  intros w l x Hxw. induction l as [| h t IH]; cbn; intros Hin; [exact Hin |].
  destruct (Nat.eqb_spec h w) as [-> | Hhw].
  - destruct Hin as [Hin | Hin]; [contradiction Hxw; symmetry; exact Hin | exact Hin].
  - destruct Hin as [-> | Hin]; [left; reflexivity | right; apply IH; exact Hin].
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

(* ── 3. The waiter side ──────────────────────────────────────────────────── *)

Section Waiters.
  (* Payloads, patterns and guards. *)
  Variables V P G : Type.
  (* The pattern and the guard of the waiter with the key w. *)
  Variable pattern : nat -> P.
  Variable guard : nat -> G.
  (* The metered spatial match and the guard (SpaceMatcher). *)
  Variable spatial : P -> V -> bool.
  Variable admits : G -> V -> bool.
  (* The pattern equality of the legacy rule. Every pattern equals itself. *)
  Variable same_pattern : P -> P -> bool.
  Hypothesis same_pattern_refl : forall p, same_pattern p p = true.

  (* The legacy removal: the first waiter whose pattern equals the pattern of
     the fired waiter [w]. *)
  Fixpoint remove_by_pattern (w : nat) (ws : list nat) : list nat :=
    match ws with
    | [] => []
    | h :: t => if same_pattern (pattern h) (pattern w) then t else h :: remove_by_pattern w t
    end.

  (* The removal of a fired waiter in the code: by its identity, at its place
     in the staged view (remove_continuation). *)
  Definition fire_remove (w : nat) (ws : list nat) : list nat := remove_one w ws.

  (* F12 waiter_removal_by_record_identity: in a side without duplicates
     (F14), the fire of [w] removes [w] and keeps every other waiter, whatever
     their patterns (D10). *)
  Theorem waiter_removal_by_record_identity : forall w ws,
    NoDup ws -> In w ws ->
    Permutation ws (w :: fire_remove w ws) /\ ~ In w (fire_remove w ws) /\
    (forall v, v <> w -> In v ws <-> In v (fire_remove w ws)).
  Proof.
    intros w ws Hnd Hin. unfold fire_remove.
    pose proof (remove_one_cons_perm w ws Hin) as Hp.
    split; [exact Hp |]. split.
    - pose proof (Permutation_NoDup Hp Hnd) as Hc. apply NoDup_cons_iff in Hc as [Hc _]. exact Hc.
    - intros v Hvw. split; [apply remove_one_keeps; exact Hvw | apply remove_one_incl].
  Qed.

  (* F13 pattern_equality_removal_can_remove_another_waiter: two waiters with
     equal patterns, with the other waiter [v] parked in front of the fired
     waiter [w]. The legacy removal keeps the fired waiter and removes [v],
     where the removal by identity removes [w]. *)
  Theorem pattern_equality_removal_can_remove_another_waiter : forall w v,
    v <> w -> pattern v = pattern w ->
    remove_by_pattern w [v; w] = [w] /\ ~ In v (remove_by_pattern w [v; w]) /\
    fire_remove w [v; w] = [v].
  Proof.
    intros w v Hvw Hpat. unfold fire_remove. cbn [remove_by_pattern remove_one].
    rewrite Hpat, same_pattern_refl.
    destruct (Nat.eqb_spec v w) as [Heq | _]; [contradiction |].
    rewrite Nat.eqb_refl. split; [reflexivity |]. split; [| reflexivity].
    intros [Heq | []]. exact (Hvw (eq_sym Heq)).
  Qed.

  (* F14: the park of the waiter [w]: nothing when a waiter with its identity
     is parked, and otherwise [w] in front. *)
  Definition park (w : nat) (ws : list nat) : list nat :=
    if existsb (Nat.eqb w) ws then ws else w :: ws.

  (* F14 park_byte_identity_idempotent: an equal park changes nothing, a new
     waiter is added in front, and a side stays without duplicates. *)
  Theorem park_byte_identity_idempotent : forall w ws,
    (In w ws -> park w ws = ws) /\ (~ In w ws -> park w ws = w :: ws) /\
    (NoDup ws -> NoDup (park w ws)).
  Proof.
    intros w ws. unfold park.
    assert (Hex : existsb (Nat.eqb w) ws = true <-> In w ws).
    { rewrite existsb_exists. split.
      - intros (x & Hx & Hwx). apply Nat.eqb_eq in Hwx. subst x. exact Hx.
      - intros Hw. exists w. split; [exact Hw | apply Nat.eqb_refl]. }
    split; [| split].
    - intros Hw. apply Hex in Hw. rewrite Hw. reflexivity.
    - intros Hw. destruct (existsb (Nat.eqb w) ws) eqn:E; [| reflexivity].
      exfalso. apply Hw. apply Hex. reflexivity.
    - intros Hnd. destruct (existsb (Nat.eqb w) ws) eqn:E; [exact Hnd |].
      constructor; [intros Hw; apply Hex in Hw; discriminate | exact Hnd].
  Qed.

  (* The waiter [w] takes the payload [x]: its pattern binds [x] and its guard
     admits it. *)
  Definition fits (x : V) (w : nat) : bool := spatial (pattern w) x && admits (guard w) x.

  (* OFFER-SCAN: the first waiter in canonical order that fits. A waiter
     whose pattern binds [x] but whose guard refuses it is skipped. *)
  Definition offer_choice (x : V) (ws : list nat) : option nat := find (fits x) (visit ws).

  (* F15 waiter_visit_order_canonical: the waiter order is sorted by key,
     holds the waiters, and does not depend on the list order of the store.
     A send fires the first waiter in that order that fits, so every earlier
     waiter fails its pattern or its guard (decision 37, the produce side). *)
  Theorem waiter_visit_order_canonical : forall x ws ws',
    Sorted le (visit ws) /\ Permutation ws (visit ws) /\
    (Permutation ws ws' -> offer_choice x ws = offer_choice x ws') /\
    (forall w, offer_choice x ws = Some w <->
       exists pre post, visit ws = pre ++ w :: post /\
         (forall j, In j pre -> fits x j = false) /\ fits x w = true).
  Proof.
    intros x ws ws'. unfold offer_choice, visit.
    split; [apply canon_sorted |]. split; [apply canon_perm |]. split.
    - intros Hp. rewrite (canon_permutation_invariant ws ws' Hp). reflexivity.
    - intros w. apply find_some_split.
  Qed.
End Waiters.

(* ── 4. Head-only and scan sides ─────────────────────────────────────────── *)

(* The waiter side of a discipline: a scan side (bag, set) offers every
   waiter in canonical order, and a head-only side (queue, stack, priority
   queue level, cell) offers its head only (decision 11). *)
Inductive side_kind := Scan | Head.

Definition candidates (kind : side_kind) (ws : list nat) : list nat :=
  match kind with
  | Scan => visit ws
  | Head => firstn 1 (visit ws)
  end.

(* F16 head_waiter_unique: a head-only side offers at most one candidate. It
   is the least waiter of the side, and a side with waiters offers one. A
   scan side offers every waiter. *)
Theorem head_waiter_unique : forall ws,
  length (candidates Head ws) <= 1 /\
  (forall h, candidates Head ws = [h] -> In h ws /\ forall w, In w ws -> h <= w) /\
  (ws <> [] -> candidates Head ws <> []) /\
  Permutation ws (candidates Scan ws).
Proof.
  intros ws. cbn [candidates]. unfold visit.
  pose proof (canon_perm ws) as Hp. pose proof (canon_sorted ws) as Hs.
  split; [| split; [| split]].
  - destruct (canon ws) as [| a t]; cbn; lia.
  - intros h Hh. destruct (canon ws) as [| a t] eqn:Hc; cbn in Hh; [discriminate |].
    injection Hh as <-. split.
    + eapply Permutation_in; [symmetry; exact Hp | left; reflexivity].
    + intros w Hw. apply (Permutation_in w Hp) in Hw. destruct Hw as [-> | Hw]; [lia |].
      exact (sorted_le_all a t Hs w Hw).
  - intros Hne. destruct (canon ws) as [| a t] eqn:Hc; cbn; [| discriminate].
    apply Permutation_sym, Permutation_nil in Hp. contradiction.
  - exact Hp.
Qed.

Print Assumptions waiter_removal_by_record_identity.
Print Assumptions pattern_equality_removal_can_remove_another_waiter.
Print Assumptions park_byte_identity_idempotent.
Print Assumptions waiter_visit_order_canonical.
Print Assumptions head_waiter_unique.
