(* ═══════════════════════════════════════════════════════════════════════════
   HeadOnlyDisciplines.v — head-only matching of the queue discipline (P4)

   This module models the queue discipline of the native spaces (plan
   decision 11) and the plans of GenericSpace (P4 design B.4). A queue is
   head-only on both sides. The head of the records and the head of the
   waiting continuations are the entries with the least (ord, bincode
   bytes), and only the two heads interact. A head that does not match
   blocks the entries behind it. An incoming element joins the tail of its
   side, so it interacts only when its side is empty. Every operation takes
   the value of the collection counter as its ord (M-1, decision 31).

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
   earlier waiters, then meets the head records one by one.

   Sources
   - Design: the P4 design report B.4 (plans, the invariant INV, the loop
     bound) and E.1 (the headlines of this module, the queue part).
   - Plan: decisions 11 (head-only), 30 (re-offer of a surviving datum) and
     31 (every operation bumps the counter), and revision I-21
     (FIRE_ROUND_CAP = 64).
   - Code: the P4 commit of this repository. The line numbers below are the
     line numbers of that commit.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   precedes, head_from, head
                            spaces/disciplines/queue.rs:32-48, 56-65 (head:
                            the least (ord, bytes), the earlier index on a
                            tie, and precedes)
   reaches_head             spaces/disciplines/queue.rs:51 (queued == 0)
   fire_round_cap, after_fire
                            reified/costs.rs:28, 116-127 (FIRE_ROUND_CAP =
                            64, FireRounds::after_fire)
   offer, offer_loop        spaces/generic.rs:337-401 (GenericSpace::offer)
   receive, receive_loop    spaces/generic.rs:405-472 (GenericSpace::receive)
   head_waiter, head_record spaces/generic.rs:300-315, 318-333
   bump                     spaces/counter.rs:118-182 (the ord is the
                            counter value, and the first operation takes 0
                            and opens the counter at 1)
   produce, consume         spaces/generic.rs:537-595 (prepare_produce and
                            prepare_consume: bump, then plan)
   legacy_offer             the legacy waiter scan (the legacy
                            GenericRSpace find_fireable_continuation_with_
                            similarity scans every waiter, commit ff479042)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - fits w d is the metered match of the payload of d against the pattern
     of w followed by the guard of w (SpaceMatcher::matches,
     spaces/matching.rs:69-82). The model takes it as a section variable:
     every theorem holds for every matcher.
   - The bincode bytes of an entry give a strict total order on distinct
     entries. The model writes rank for the position of those bytes in that
     order. Two entries with one (ord, rank) are byte-identical. The premise
     key_injective of the permutation theorem states this, and the corollary
     plan_permutation_invariant_instance discharges it.
   - The staged view of a collection is a record list and a waiter list. A
     fire removes the fired partner at its view index unless it stays.
   - The ord is a nat. The code checks i64 overflow with CounterOverflow
     (spaces/counter.rs:133, 157), so every ord of the code is a nat.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. queue_head_is_min_ord             the head has the least (ord, rank)
    2. queue_produce_preserves_inv       a produce keeps INV
    3. queue_consume_preserves_inv       a consume keeps INV
    4. queue_fifo                        records leave in arrival order
    5. queue_head_blocks                 a blocked head blocks the rest
    6. queue_loop_bounded_or_reissues    at most FIRE_ROUND_CAP fires, at
                                         most one more than the queue length
    7. queue_ord_fresh                   the ord of an operation is new
    8. queue_plan_permutation_invariant  the plan ignores the list order
    9. legacy_scan_violates_head_blocks  the legacy scan breaks head-only

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

  (* GenericSpace::offer: a datum joins the tail when a record is queued. *)
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

  (* GenericSpace::receive: a continuation joins the tail when a waiter is
     queued. *)
  Definition receive (st : qstate) (k : waiter) : outcome :=
    match q_waiters st with
    | _ :: _ => mkOutcome [SPark k] (park_waiter st k) None
    | [] => receive_loop fire_round_cap 0 st k
    end.

  (* prepare_produce and prepare_consume: bump the counter, then plan with
     the ord of the bump. rank is the byte rank of the new entry. *)
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

  (* Headline 7. The ord of an operation is the counter value, which is
     above the ord of every entry of the collection. The operation
     advances the counter by one and keeps the collection well formed. The
     first operation takes the ord 0 and opens the counter at 1. *)
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

  (* A run: each operation in its own transaction. The second component is
     the list of the records that the run removed, in removal order. A
     re-issued operation is an operation of the list. *)
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

  (* Headline 4. Over any run of operations from a well-formed collection,
     the records leave in strictly increasing ord, every removed record is
     older than every record that stays, and a removed record was in the
     collection at the start or arrived during the run. Since a produce
     takes the counter value as the ord of its record, the removal order is
     the arrival order. *)
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
End Queue.

(* ── 14. Headline 9: the legacy scan breaks head-only ────────────────── *)

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

(* ── 15. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions queue_head_is_min_ord.
Print Assumptions queue_produce_preserves_inv.
Print Assumptions queue_consume_preserves_inv.
Print Assumptions queue_fifo.
Print Assumptions queue_head_blocks.
Print Assumptions queue_loop_bounded_or_reissues.
Print Assumptions queue_ord_fresh.
Print Assumptions queue_plan_permutation_invariant.
Print Assumptions legacy_scan_violates_head_blocks.
