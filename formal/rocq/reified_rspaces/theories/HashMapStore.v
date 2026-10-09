(* ═══════════════════════════════════════════════════════════════════════════
   HashMapStore.v — the collections of a HashMap-keyed store (P5d, P7)

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

   The HashSet store of P7 keys its collections as the HashMap store does,
   and it restricts the operations to units (design v3, section 6, HS-A of
   decision 73). Section 5 proves its unit rules (hashset_unit_rules): the
   payload check accepts exactly the empty payload (HS-2), the pattern check
   accepts exactly the empty pattern list with no remainder and no binder
   (HS-3), an empty pattern list matches exactly the empty payload (the
   fold_match rule), both checks fail before any staging, and so every record
   of a HashSet store is a unit. The qualifier rule of HashSet (seq only,
   HS-6) is headline 3 of SpaceFactory.v, qualifier_matrix.

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
     view of a collection), decision 73 (HashSet keeps bag semantics on
     units, HS-A).
   - Design (P7): design v3 section 6 (HS-1 to HS-8) and section 9 (the
     HashSet model, with a negative check).
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
   check_payload            spaces/outer/hashset.rs, HashSetOuter::
                            check_payload (HS-2)
   check_patterns, bind     spaces/outer/hashset.rs, HashSetOuter::
                            check_patterns, over models::rhoapi::BindPattern
                            (patterns, remainder, free_count) (HS-3)
   match_list               matcher/fold_match.rs, FoldMatch::fold_match
                            for Par, through matcher/match.rs,
                            Matcher::get_with_context
   hs_send, hs_receive      spaces/generic.rs, the checks of run_with and
                            spaces/join.rs, validate: the hooks run after
                            check_key and before any staging; a take of the
                            first matching record stands for a fire

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
    3. hashset_unit_rules        the HashSet checks accept exactly units and
                                 empty binds, an empty bind matches exactly
                                 a unit, a rejected operation stages
                                 nothing, and every record of a HashSet
                                 store is a unit (P7, HS-2 and HS-3)

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - The legacy model represented the store as a function from channels to
     lists and proved store equalities with functional extensionality. That
     is an axiom. Pointwise statements over every key replace it.

   ─────────────────────────────────────────────────────────────────────────
   Abstraction of section 5 (the HashSet store)
   ─────────────────────────────────────────────────────────────────────────
   - A record of a HashSet collection is its payload, a list of values: the
     envelope of a record is the same for every store (RecordEnvelope.v), so
     the model leaves it out.
   - A pattern, a value, a remainder and the match of one pattern against one
     value are section variables (match_one), as the spatial matcher is. A
     remainder absorbs the surplus values when absorb says so: a free
     variable runs free_check and a wildcard absorbs every list.
   - A waiter is not a record. A receive that matches no record changes no
     record of the store, so the model leaves the store as it is.
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

(* ── 5. The HashSet store (P7, HS-1 to HS-5) ────────────────────────────── *)

Section HashSetStore.
  Variable K : Type.
  Variable key_eq_dec : forall x y : K, {x = y} + {x <> y}.
  (* A value of a payload, a pattern, and a remainder variable. *)
  Variables V Pat Rem : Type.
  (* The spatial match of one pattern against one value. *)
  Variable match_one : Pat -> V -> bool.
  (* A remainder that absorbs the surplus values: free_check for a free
     variable, every list for a wildcard. *)
  Variable absorb : Rem -> list V -> bool.

  (* A record of a HashSet collection: its payload. *)
  Definition payload : Type := list V.

  (* models::rhoapi::BindPattern: the patterns, the remainder and the number
     of binders (free_count). *)
  Record bind : Type := mkBind { patterns : list Pat; remainder : option Rem; binders : nat }.

  (* The two codes of HS-8. *)
  Inductive hs_error : Type := PayloadNotEmpty | PatternNotEmpty.

  (* HS-2: HashSetOuter::check_payload. *)
  Definition check_payload (p : payload) : bool :=
    match p with [] => true | _ :: _ => false end.

  (* HS-3: HashSetOuter::check_patterns. *)
  Definition check_patterns (b : bind) : bool :=
    match patterns b, remainder b, binders b with
    | [], None, 0 => true
    | _, _, _ => false
    end.

  (* fold_match for Par: the patterns meet the values in order. Surplus
     values need a remainder that absorbs them, and surplus patterns never
     match. *)
  Fixpoint match_list (ps : list Pat) (rem : option Rem) (vs : list V) : bool :=
    match ps, vs with
    | [], [] => true
    | [], _ :: _ => match rem with None => false | Some r => absorb r vs end
    | _ :: _, [] => false
    | p :: ps', v :: vs' => match_one p v && match_list ps' rem vs'
    end.

  (* A send on the collection k: the payload check before any staging, then
     the store of the unit. *)
  Definition hs_send (k : K) (p : payload) (s : store K payload) : store K payload + hs_error :=
    if check_payload p then inl (put K payload k p s) else inr PayloadNotEmpty.

  (* A receive on the collection k: the pattern check before any staging,
     then the take of the first record that the bind matches. A receive that
     matches no record leaves every record in place. *)
  Definition hs_receive (k : K) (b : bind) (s : store K payload) : store K payload + hs_error :=
    if check_patterns b
    then inl (take K key_eq_dec payload k (match_list (patterns b) (remainder b)) s)
    else inr PatternNotEmpty.

  Inductive hs_op : Type := Send (k : K) (p : payload) | Receive (k : K) (b : bind).

  (* A sequence of operations from the store s. The first rejected
     operation ends the sequence with its error. *)
  Fixpoint hs_run (ops : list hs_op) (s : store K payload) : store K payload + hs_error :=
    match ops with
    | [] => inl s
    | Send k p :: rest => match hs_send k p s with inl s' => hs_run rest s' | inr e => inr e end
    | Receive k b :: rest =>
        match hs_receive k b s with inl s' => hs_run rest s' | inr e => inr e end
    end.

  (* Every record of the store s is a unit. *)
  Definition units_only (s : store K payload) : Prop := forall e, In e s -> snd e = [].

  Lemma take_incl : forall k p s e, In e (take K key_eq_dec payload k p s) -> In e s.
  Proof.
    intros k p s e. induction s as [| [k0 r] rest IH]; [intros H; exact H |].
    simpl. destruct (key_eq_dec k0 k) as [_ | _].
    - destruct (p r); intros H; [right; exact H |].
      destruct H as [H | H]; [left; exact H | right; exact (IH H)].
    - intros H. destruct H as [H | H]; [left; exact H | right; exact (IH H)].
  Qed.

  Lemma get_in : forall k s r, In r (get K key_eq_dec payload k s) -> In (k, r) s.
  Proof.
    intros k s r H. unfold get in H. apply in_map_iff in H as [[k' r'] [Hr Hin]].
    cbn in Hr. subst r'. apply filter_In in Hin as [Hin Hkey].
    unfold is_key in Hkey. cbn in Hkey.
    destruct (key_eq_dec k' k) as [-> | _]; [exact Hin | discriminate].
  Qed.

  Lemma hs_run_units : forall ops s s', units_only s -> hs_run ops s = inl s' -> units_only s'.
  Proof.
    intros ops. induction ops as [| op rest IH]; intros s s' units ran.
    - cbn in ran. injection ran as <-. exact units.
    - destruct op as [k p | k b]; cbn [hs_run] in ran.
      + unfold hs_send in ran. destruct p as [| v vs]; cbn in ran; [| discriminate].
        refine (IH _ _ _ ran). intros e He. cbn in He.
        destruct He as [He | He]; [subst e; reflexivity | exact (units e He)].
      + unfold hs_receive in ran. destruct (check_patterns b); cbn in ran; [| discriminate].
        refine (IH _ _ _ ran). intros e He. exact (units e (take_incl _ _ _ _ He)).
  Qed.

  (* Headline 3 (P7, HS-2 and HS-3). The payload check accepts exactly the
     empty payload, and the pattern check accepts exactly the empty pattern
     list with no remainder and no binder. An empty bind matches exactly the
     empty payload. A rejected send or receive stages nothing: it ends with
     its code and no store. So every store that accepted operations make from
     the empty store holds units only, in every collection. *)
  Theorem hashset_unit_rules :
    (forall p, check_payload p = true <-> p = []) /\
    (forall b, check_patterns b = true <-> patterns b = [] /\ remainder b = None /\ binders b = 0) /\
    (forall b vs, check_patterns b = true -> (match_list (patterns b) (remainder b) vs = true <-> vs = [])) /\
    (forall k p s, check_payload p = false -> hs_send k p s = inr PayloadNotEmpty) /\
    (forall k b s, check_patterns b = false -> hs_receive k b s = inr PatternNotEmpty) /\
    (forall ops s k r, hs_run ops [] = inl s -> In r (get K key_eq_dec payload k s) -> r = []).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros [| v vs]; cbn; split; intros H; solve [reflexivity | discriminate].
    - intros [ps rem n]. unfold check_patterns. cbn.
      destruct ps as [| p ps], rem as [r |], n as [| n]; cbn; split;
        solve [ intros H; discriminate H
              | intros [H1 [H2 H3]]; discriminate
              | intros _; repeat split
              | intros _; reflexivity ].
    - intros [ps rem n] vs H. unfold check_patterns in H. cbn in H |- *.
      destruct ps as [| p ps], rem as [r |], n as [| n]; try discriminate.
      destruct vs as [| v vs]; cbn; split; intros H'; solve [reflexivity | discriminate].
    - intros k p s H. unfold hs_send. rewrite H. reflexivity.
    - intros k b s H. unfold hs_receive. rewrite H. reflexivity.
    - intros ops s k r ran Hr.
      assert (units : units_only s).
      { apply (hs_run_units ops [] s); [intros e [] | exact ran]. }
      exact (units (k, r) (get_in k s r Hr)).
  Qed.
End HashSetStore.

(* A computed example: a unit is stored and taken, and a payload or a
   binder fails with its code. Keys and values are numbers, and every value
   matches every pattern. *)
Example hashset_example :
  let m := fun (_ : nat) (_ : nat) => true in
  let a := fun (_ : nat) (_ : list nat) => true in
  hs_run nat Nat.eq_dec nat nat nat m a
    [Send nat nat nat nat 1 []; Receive nat nat nat nat 1 (mkBind nat nat [] None 0)] [] = inl [] /\
  hs_run nat Nat.eq_dec nat nat nat m a [Send nat nat nat nat 1 [7]] [] = inr PayloadNotEmpty /\
  hs_run nat Nat.eq_dec nat nat nat m a
    [Receive nat nat nat nat 1 (mkBind nat nat [3] None 1)] [] = inr PatternNotEmpty.
Proof. repeat split; reflexivity. Qed.

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
Print Assumptions hashset_unit_rules.
