(* ═══════════════════════════════════════════════════════════════════════════
   VectorStore.v — the index names of a Vector space (P5d)

   This module models the allocator of a Vector space (P5 design v3 A.10,
   design v2 section 3.3, F8 and F11). A Vector space has no bound on its
   names other than the bound of its allocator record. The record A(sid)
   counts the names that the space allocated. An allocation of k names from
   the value n returns the indices n, n + 1, ..., n + k − 1 and puts
   A(sid) = n + k, when n + k is at most i64::MAX. Otherwise it fails with
   OutOfNames and allocates nothing. The handle method gensym is an
   allocation of one name. The store accepts every index name of its own
   space, and no other channel (F8).

   The legacy vector store (f1r3node-reified-rspaces, ChannelStore/
   VectorStore.v and vector_store.rs) kept its next index in memory: its
   gensym returned the length of an in-memory list, so a restart lost it.
   This module restates its three headlines for the allocator record.

   Sources
   - Design: P5 design v3 A.10 (ALLOC, vector), A.13 (VectorStore.v 3);
     P5 design v2 section 3.3 (the headlines vector_index_is_counter,
     vector_gensym_monotonic and vector_gensym_distinct).
   - Plan: decision 39 (F8, F11).
   - Code: the P5 W2 tree of this repository. The paths below are relative
     to rholang/src/rust/interpreter.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   vmax                     i64::MAX, the bound of a counter record
                            (spaces/record.rs, counter_value: a natural i64)
   vplan                    spaces/allocator.rs (AllocationRule::plan, the
                            arm Unbounded: bounded_sum, then sequential)
   vafter, vrun, vfinal     spaces/allocator.rs (fn allocate: the take of
                            A(sid) = n and the put of A(sid) = next) and
                            spaces/generic.rs (GenericSpace::run_allocate)
   vallocated               the indices of a run of allocation jobs
   channel, vcheck_key      spaces/allocator.rs (index_of_name) and
                            spaces/outer/vector.rs (VectorOuter::check_key)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A value and an index are natural numbers, and vmax is a section
     variable for i64::MAX. The code computes n + k with checked_add in
     i64, which fails exactly when the model's n + k passes vmax.
   - A space and a random tail are natural numbers, and a forged channel is
     a number (the decoder of the code is the constructor of the channel).
   - The staged take and put of a job, their identities and the metering of
     a job are the same as for an Array space. OuterStorage.v proves them
     for every rule, so this module states only the vector rule.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. vector_index_is_counter   the allocator counts the names: a run from
                                 0 allocates 0, 1, ..., final − 1, each
                                 allocation returns n..n+k−1 and puts n + k,
                                 and the store accepts every index name of
                                 its own space
    2. vector_gensym_distinct    a run never allocates one index twice
    3. vector_gensym_monotonic   a run of gensyms from n returns n, n + 1,
                                 n + 2, ..., each one more than the last

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - The legacy module imported a Prelude and a DataCollection module, so
     it was not standalone. Its storage list does not exist in the reified
     store, which keeps no list of collections. The model keeps only the
     allocator value.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
Import ListNotations.

Section Vector.
  (* The greatest value of the allocator record: i64::MAX in the code. *)
  Variable vmax : nat.

  (* ── 1. The vector rule ──────────────────────────────────────────────── *)

  (* One allocation of k names from the value n: the indices and the next
     value, or None for OutOfNames. *)
  Definition vplan (n k : nat) : option (list nat * nat) :=
    if n + k <=? vmax then Some (seq n k, n + k) else None.

  (* The allocator value after the commit of one allocation. *)
  Definition vafter (n k : nat) : nat :=
    match vplan n k with
    | Some (_, next) => next
    | None => n
    end.

  (* A run of allocations of the counts on one space from the value n. *)
  Fixpoint vrun (n : nat) (counts : list nat) : list (option (list nat * nat)) :=
    match counts with
    | [] => []
    | k :: rest => vplan n k :: vrun (vafter n k) rest
    end.

  Fixpoint vfinal (n : nat) (counts : list nat) : nat :=
    match counts with
    | [] => n
    | k :: rest => vfinal (vafter n k) rest
    end.

  (* Every index that a run allocates, in allocation order. *)
  Definition vallocated (outs : list (option (list nat * nat))) : list nat :=
    flat_map (fun o => match o with Some (idx, _) => idx | None => [] end) outs.

  Lemma vallocated_cons : forall o outs,
    vallocated (o :: outs) =
      match o with Some (idx, _) => idx | None => [] end ++ vallocated outs.
  Proof. reflexivity. Qed.

  Lemma vplan_fits : forall n k, n + k <= vmax -> vplan n k = Some (seq n k, n + k).
  Proof.
    intros n k fits. unfold vplan.
    destruct (Nat.leb_spec (n + k) vmax) as [_ | over]; [reflexivity | lia].
  Qed.

  Lemma vplan_full : forall n k, vmax < n + k -> vplan n k = None.
  Proof.
    intros n k full. unfold vplan.
    destruct (Nat.leb_spec (n + k) vmax) as [fits | _]; [lia | reflexivity].
  Qed.

  (* ── 2. The key check (F8) ──────────────────────────────────────────── *)

  (* A channel: an index name of a space, a random-tail name of a space, or
     any other channel. *)
  Inductive channel : Type :=
  | IndexName (space index : nat)
  | RandomName (space tail : nat)
  | Forged (c : nat).

  Definition vcheck_key (space : nat) (c : channel) : bool :=
    match c with
    | IndexName sp _ => Nat.eqb sp space
    | RandomName _ _ | Forged _ => false
    end.

  Lemma vcheck_key_spec : forall space c,
    vcheck_key space c = true <-> exists i, c = IndexName space i.
  Proof.
    intros space c. split.
    - destruct c as [sp i | sp t | f]; simpl; try discriminate.
      intros same. apply Nat.eqb_eq in same. subst sp. exists i. reflexivity.
    - intros [i ->]. simpl. apply Nat.eqb_refl.
  Qed.

  (* ── 3. The counter ─────────────────────────────────────────────────── *)

  (* From a value within the bound, a run allocates n, n + 1, ... up to its
     final value, which stays within the bound. *)
  Lemma vrun_seq : forall counts n,
    n <= vmax ->
    vallocated (vrun n counts) = seq n (vfinal n counts - n) /\
    n <= vfinal n counts <= vmax.
  Proof.
    intros counts. induction counts as [| k rest IH]; intros n within.
    - simpl. rewrite Nat.sub_diag. split; [reflexivity | lia].
    - change (vrun n (k :: rest)) with (vplan n k :: vrun (vafter n k) rest).
      change (vfinal n (k :: rest)) with (vfinal (vafter n k) rest).
      rewrite vallocated_cons.
      destruct (Nat.leb_spec (n + k) vmax) as [fits | full].
      + assert (moved : vafter n k = n + k)
          by (unfold vafter; rewrite vplan_fits by exact fits; reflexivity).
        rewrite vplan_fits by exact fits. rewrite moved.
        destruct (IH (n + k) fits) as [same bounds].
        rewrite same. split; [| lia].
        replace (vfinal (n + k) rest - n) with (k + (vfinal (n + k) rest - (n + k))) by lia.
        rewrite seq_app. reflexivity.
      + assert (stays : vafter n k = n)
          by (unfold vafter; rewrite vplan_full by exact full; reflexivity).
        rewrite vplan_full by exact full. rewrite stays.
        destruct (IH n within) as [same bounds].
        rewrite same. split; [reflexivity | lia].
  Qed.

  Theorem vector_index_is_counter : forall counts space,
    vallocated (vrun 0 counts) = seq 0 (vfinal 0 counts) /\
    vfinal 0 counts = length (vallocated (vrun 0 counts)) /\
    vfinal 0 counts <= vmax /\
    (forall n k idx next, vplan n k = Some (idx, next) -> idx = seq n k /\ next = n + k) /\
    (forall c, vcheck_key space c = true <-> exists i, c = IndexName space i).
  Proof.
    intros counts space.
    destruct (vrun_seq counts 0 (Nat.le_0_l vmax)) as [same bounds].
    rewrite Nat.sub_0_r in same.
    split; [exact same |]. split; [rewrite same, length_seq; reflexivity |].
    split; [lia |]. split.
    - intros n k idx next planned.
      destruct (Nat.leb_spec (n + k) vmax) as [fits | full].
      + rewrite (vplan_fits n k fits) in planned.
        injection planned as <- <-. split; reflexivity.
      + rewrite (vplan_full n k full) in planned. discriminate.
    - intros c. apply vcheck_key_spec.
  Qed.

  Theorem vector_gensym_distinct : forall counts n,
    n <= vmax -> NoDup (vallocated (vrun n counts)).
  Proof.
    intros counts n within.
    destruct (vrun_seq counts n within) as [same _].
    rewrite same. apply seq_NoDup.
  Qed.

  (* ── 4. Gensyms ─────────────────────────────────────────────────────── *)

  Lemma vafter_fits : forall n k, n + k <= vmax -> vafter n k = n + k.
  Proof. intros n k fits. unfold vafter. rewrite vplan_fits by exact fits. reflexivity. Qed.

  (* The handle method gensym allocates one name. A run of m gensyms from n
     returns [n], [n + 1], ..., and each puts the next value. *)
  Theorem vector_gensym_monotonic : forall m n,
    n + m <= vmax ->
    vrun n (repeat 1 m) = map (fun i => Some ([i], S i)) (seq n m).
  Proof.
    intros m. induction m as [| m IH]; intros n fits; [reflexivity |].
    change (repeat 1 (S m)) with (1 :: repeat 1 m).
    change (vrun n (1 :: repeat 1 m)) with (vplan n 1 :: vrun (vafter n 1) (repeat 1 m)).
    rewrite vplan_fits, vafter_fits by lia.
    rewrite Nat.add_1_r. rewrite IH by lia. reflexivity.
  Qed.

  (* Three gensyms on a fresh space, and an allocation that passes the
     bound of a space whose bound is 5. *)
  Example vector_examples :
    (3 <= vmax -> vallocated (vrun 0 [1; 1; 1]) = [0; 1; 2]) /\ (vmax = 5 -> vplan 4 2 = None).
  Proof.
    split.
    - intros fits. change [1; 1; 1] with (repeat 1 3).
      rewrite vector_gensym_monotonic by (simpl; lia). reflexivity.
    - intros five. unfold vplan. rewrite five. reflexivity.
  Qed.

End Vector.

(* ── 5. Assumption audit ─────────────────────────────────────────────── *)

Print Assumptions vector_index_is_counter.
Print Assumptions vector_gensym_distinct.
Print Assumptions vector_gensym_monotonic.
