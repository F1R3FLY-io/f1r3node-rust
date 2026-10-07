(* ═══════════════════════════════════════════════════════════════════════════
   HashMapStore.v — the collections of a HashMap-keyed store (P5d)

   The HashMap, Array and Vector outer stores keep each collection of a
   space on its own data channel D(sid, k), where the key k is the canonical
   bytes of the channel of the collection (A5). The three stores share the
   key and the channels, so the store is a map from keys to collections.
   This module proves the two map laws that the stores need:

   - a record stored in one collection, or removed from it, leaves every
     other collection as it was (hashmap_get_put_other);
   - the content of every collection, read in an order-free way, does not
     depend on the order of the stores, and an iteration over the keys in a
     canonical order sees the same collections (hashmap_iteration_permutation_
     invariant).

   The legacy HashMap store (f1r3node-reified-rspaces, ChannelStore/
   HashMapStore.v and hash_map_bag_channel_store.rs) iterated a Rust HashMap
   in its hash order and imported the FunctionalExtensionality axiom to
   compare two stores as functions. This module needs no axiom: it states
   the laws pointwise, for every key, and up to permutation. It does not
   restate the injectivity of the keys and channels (Namespace.v,
   storage_id_injective and canon_respects_par_eq).

   Sources
   - Design: P5 design v3 A.10 (the three stores share the key), A.13
     (HashMapStore.v 2); P5 design v2 section 3.3 (the headlines
     hashmap_iteration_permutation_invariant and hashmap_get_put_other,
     without restating Namespace.v Q2 and Q11).
   - Plan: decision 32 (A5: the key is the bincode of the sorted Par),
     decision 41 (bag and set order candidates by source hash, an order-free
     view of a collection).
   - Code: the P5 W2 tree of this repository. The paths below are relative
     to rholang/src/rust/interpreter.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   K, key_eq_dec            namespace::CollectionKey (the canonical bytes,
                            compared bytewise); spaces/outer/hashmap.rs,
                            array.rs and vector.rs (OuterStore::key, which
                            the Array and Vector stores delegate to the
                            HashMap store)
   store, put, get          the records of the data channels D(sid, k) in
                            the host store: a stored produce on D(sid, k)
                            (Cx::store, generic.rs) and the staged view
                            (Cx::data, generic.rs, the hot store inserts at
                            the front of a channel)
   take                     a fire that removes one stored record of one
                            collection (Cx::fire_receive, generic.rs)
   order_free, iterate      the canonical orders of the readers: the ord of
                            an envelope (Cx::head_datum) and the source-hash
                            order of DR-75 (Cx::canonical_data_order)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - The store of one space is a list of (key, record) pairs, newest first,
     as the hot store keeps the data of a channel. The key of a pair is the
     key of its data channel. The channel of a key is injective
     (Namespace.v), so the model keys the records by the key directly.
   - A reader sees a collection through a view that does not depend on the
     list order of the records (order_free): every agent read goes through
     a canonical order, never through the list order of the store.
   - Waiters live on the same data channels and obey the same laws. The
     model names their entries records too.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. hashmap_iteration_permutation_invariant
                                 permuted stores give permuted collections,
                                 and every order-free iteration is equal
    2. hashmap_get_put_other     a store or a removal on one key leaves
                                 every other key as it was, and a store
                                 adds its record to its own key

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - The legacy model represented the store as a function from channels to
     lists and proved store equalities with functional extensionality. That
     is an axiom. Pointwise statements over every key replace it.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section Store.
  (* The key of a collection, with a decidable equality: the canonical
     bytes of its channel, compared bytewise. *)
  Variable K : Type.
  Variable key_eq_dec : forall x y : K, {x = y} + {x <> y}.
  (* A record, or a waiter, of a collection. *)
  Variable R : Type.

  (* ── 1. The store ──────────────────────────────────────────────────── *)

  (* The records of one space, each with the key of its collection, newest
     first. *)
  Definition store : Type := list (K * R).

  Definition is_key (k : K) (entry : K * R) : bool :=
    if key_eq_dec (fst entry) k then true else false.

  (* The records of the collection k: the data channel D(sid, k). *)
  Definition get (k : K) (s : store) : list R := map snd (filter (is_key k) s).

  (* A store of the record r in the collection k. *)
  Definition put (k : K) (r : R) (s : store) : store := (k, r) :: s.

  (* A fire that removes the first record of the collection k that the
     pattern p matches. *)
  Fixpoint take (k : K) (p : R -> bool) (s : store) : store :=
    match s with
    | [] => []
    | (k', r) :: rest =>
        if key_eq_dec k' k
        then (if p r then rest else (k', r) :: take k p rest)
        else (k', r) :: take k p rest
    end.

  (* The store after the puts, in order, from the empty store. *)
  Definition build (puts : list (K * R)) : store :=
    fold_left (fun s entry => put (fst entry) (snd entry) s) puts [].

  (* A view of a collection that does not depend on the list order of its
     records: the canonical order of a discipline, for example. *)
  Definition order_free (view : list R -> list R) : Prop :=
    forall l1 l2, Permutation l1 l2 -> view l1 = view l2.

  (* An iteration over the collections of the keys keys, in their order. *)
  Definition iterate (view : list R -> list R) (keys : list K) (s : store) : list (K * list R) :=
    map (fun k => (k, view (get k s))) keys.

  (* ── 2. Permutations ───────────────────────────────────────────────── *)

  Lemma build_rev_app : forall puts acc,
    fold_left (fun s entry => put (fst entry) (snd entry) s) puts acc = rev puts ++ acc.
  Proof.
    intros puts. induction puts as [| [k r] rest IH]; intros acc; [reflexivity |].
    simpl. rewrite IH. unfold put. simpl. rewrite <- app_assoc. reflexivity.
  Qed.

  Lemma build_rev : forall puts, build puts = rev puts.
  Proof. intros puts. unfold build. rewrite build_rev_app, app_nil_r. reflexivity. Qed.

  Lemma filter_permutation : forall (f : K * R -> bool) l1 l2,
    Permutation l1 l2 -> Permutation (filter f l1) (filter f l2).
  Proof.
    intros f l1 l2 permuted. induction permuted as [| x l1 l2 _ IH | x y l | l1 l2 l3 _ IH12 _ IH23].
    - constructor.
    - simpl. destruct (f x); [apply perm_skip |]; exact IH.
    - simpl. destruct (f x), (f y); try constructor; apply Permutation_refl.
    - apply (Permutation_trans IH12 IH23).
  Qed.

  Lemma get_permutation : forall k s1 s2,
    Permutation s1 s2 -> Permutation (get k s1) (get k s2).
  Proof.
    intros k s1 s2 permuted. unfold get.
    apply Permutation_map, filter_permutation. exact permuted.
  Qed.

  Theorem hashmap_iteration_permutation_invariant :
    forall (view : list R -> list R) (keys : list K) (puts1 puts2 : list (K * R)),
      order_free view ->
      Permutation puts1 puts2 ->
      (forall k, Permutation (get k (build puts1)) (get k (build puts2))) /\
      iterate view keys (build puts1) = iterate view keys (build puts2).
  Proof.
    intros view keys puts1 puts2 free permuted.
    assert (stores : Permutation (build puts1) (build puts2)).
    { rewrite !build_rev.
      apply (Permutation_trans (Permutation_sym (Permutation_rev puts1))).
      apply (Permutation_trans permuted). apply Permutation_rev. }
    assert (collections : forall k, Permutation (get k (build puts1)) (get k (build puts2))).
    { intros k. apply get_permutation. exact stores. }
    split; [exact collections |].
    unfold iterate. apply map_ext. intros k. f_equal. apply free. apply collections.
  Qed.

  (* ── 3. Other keys ─────────────────────────────────────────────────── *)

  Lemma is_key_other : forall k k' r, k <> k' -> is_key k' (k, r) = false.
  Proof.
    intros k k' r different. unfold is_key. simpl.
    destruct (key_eq_dec k k') as [same | _]; [contradiction | reflexivity].
  Qed.

  Lemma is_key_same : forall k r, is_key k (k, r) = true.
  Proof.
    intros k r. unfold is_key. simpl.
    destruct (key_eq_dec k k) as [_ | absurd]; [reflexivity | contradiction].
  Qed.

  Arguments is_key : simpl never.

  Lemma filter_take_other : forall k k' p s,
    k <> k' -> filter (is_key k') (take k p s) = filter (is_key k') s.
  Proof.
    intros k k' p s different. induction s as [| [k0 r] rest IH]; [reflexivity |].
    simpl. destruct (key_eq_dec k0 k) as [same | other].
    - subst k0. rewrite (is_key_other k k' r different).
      destruct (p r); [reflexivity |].
      simpl. rewrite (is_key_other k k' r different). exact IH.
    - simpl. destruct (is_key k' (k0, r)); [f_equal |]; exact IH.
  Qed.

  Theorem hashmap_get_put_other : forall k k' r p s,
    k <> k' ->
    get k' (put k r s) = get k' s /\
    get k' (take k p s) = get k' s /\
    get k (put k r s) = r :: get k s.
  Proof.
    intros k k' r p s different. unfold get, put. split; [| split].
    - simpl. rewrite (is_key_other k k' r different). reflexivity.
    - rewrite (filter_take_other k k' p s different). reflexivity.
    - simpl. rewrite is_key_same. reflexivity.
  Qed.

End Store.

(* A computed example over keys and records that are numbers: two orders of
   three stores give one sorted view of each collection. *)
Example hashmap_example :
  let puts1 := [(1, 10); (2, 20); (1, 11)] in
  let puts2 := [(1, 11); (1, 10); (2, 20)] in
  get nat Nat.eq_dec nat 1 (build nat nat puts1) = [11; 10] /\
  get nat Nat.eq_dec nat 1 (build nat nat puts2) = [10; 11].
Proof. split; reflexivity. Qed.

(* ── 4. Assumption audit ─────────────────────────────────────────────── *)

Print Assumptions hashmap_iteration_permutation_invariant.
Print Assumptions hashmap_get_put_other.
