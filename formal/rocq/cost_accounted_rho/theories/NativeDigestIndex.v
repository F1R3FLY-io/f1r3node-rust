(* D-S1 (epic 8946, Phase D item D-C2b; decision record DR-96): the charges
   of the digest-keyed ordered index of the native session store.

   The index (rspace++/src/rspace/hot_store/native_index.rs) is a set of
   imbl 7.0.2 OrdMap shards keyed by 32-byte digests. An OrdMap is a B+tree:
   - a leaf holds at most 16 entries, and a branch at most 16 keys and
     17 children (ORD_CHUNK_SIZE = 16, imbl config.rs:21-22);
   - the store never removes a key, so every node is built by inserts.
     A leaf split leaves 8 entries in each half and a branch split leaves 8
     keys in each half (imbl nodes/btree.rs:554-608), and a root branch has
     at least 2 children (new_from_split, btree.rs:650-664). wf below uses
     the weaker minima of 5 entries per leaf and 8 children per branch,
     which every insert-only shape satisfies;
   - a search makes one binary search per level (btree.rs:686-717) over at
     most 16 elements, which needs at most 5 comparisons (btree.rs:1300-1350).

   Results:
   - ord_size_lower_bound and ord_root_lower_bound: a non-root subtree of
     height h holds at least 5 * 8^h entries, and a root branch of height
     h + 1 holds at least 10 * 8^h entries;
   - bsearch_le_5: imbl's binary search makes at most 5 comparisons in a node
     of at most 16 elements;
   - ord_search_comparisons and ord_search_within_bound: a search makes at
     most 5 comparisons per level, and a tree with at most n entries has at
     most levels_bound n levels, so a search makes at most
     5 * levels_bound n comparisons;
   - ord_levels_bound_monotone, key_bound_levels: the bound grows with the
     size, and it is 7 for the store's key bound of 2^20 keys;
   - ord_insert_allocations_le and ord_replace_allocations_le: an insert
     allocates at most 2 * levels_bound n + 2 nodes (a path copy and a split
     sibling per level, and the transient default leaf and the new root of a
     root split, imbl ord/map.rs:704-707); a replace allocates at most
     levels_bound n nodes (path copies only);
   - per_key_schedule_total_invariant: when the charge of an operation
     depends only on the earlier operations on its own key, every schedule
     that keeps the order of each key's operations has the same total;
   - population_charge_schedule_dependent (negative control): a charge that
     reads the population of a shared shard gives two such schedules
     different totals;
   - legacy_insert_nodes_example (contrast): the legacy insert model charged
     398 nodes for a shard of 32 entries; the index charges at most 16.

   Rust correspondence: rspace++/src/rspace/hot_store/native_index.rs
   (ord_levels_bound, ORD_LEVELS, search_charge, replace_charge,
   insert_charge, DigestShards); tests in native_index/tests.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

(* ------------------------------------------------------------------------ *)
(* B+trees and their sizes                                                  *)

Inductive bptree := BLeaf (entries : list nat) | BBranch (keys : list nat) (children : list bptree).

Fixpoint size (t : bptree) : nat :=
  match t with
  | BLeaf entries => length entries
  | BBranch _ children => fold_right Nat.add 0 (map size children)
  end.

(* wf root h t: t is a B+tree of height h (leaves have height 0). [root]
   relaxes the minimum fill of the top node. *)
Fixpoint wf (root : bool) (h : nat) (t : bptree) {struct h} : Prop :=
  match h, t with
  | 0, BLeaf entries => (if root then 0 else 5) <= length entries /\ length entries <= 16
  | S lower, BBranch keys children =>
      length children = S (length keys) /\ length keys <= 16 /\
      (if root then 2 else 8) <= length children /\ Forall (wf false lower) children
  | _, _ => False
  end.

Fixpoint pow8 (h : nat) : nat := match h with 0 => 1 | S lower => 8 * pow8 lower end.

Lemma pow8_positive : forall h, 1 <= pow8 h.
Proof. induction h; simpl; lia. Qed.

Lemma pow8_monotone : forall a b, a <= b -> pow8 a <= pow8 b.
Proof.
  intros a b ab. induction ab as [| b ab IH]; [lia |].
  simpl. pose proof (pow8_positive b). lia.
Qed.

Lemma pow8_grows : forall h, S h <= 10 * pow8 h.
Proof. induction h as [| h IH]; simpl; lia. Qed.

Lemma sum_lower_bound : forall (children : list bptree) minimum,
  Forall (fun child => minimum <= size child) children ->
  length children * minimum <= fold_right Nat.add 0 (map size children).
Proof.
  intros children minimum bounded.
  induction bounded as [| child rest child_bound _ IH]; simpl; lia.
Qed.

Theorem ord_size_lower_bound : forall h t, wf false h t -> 5 * pow8 h <= size t.
Proof.
  induction h as [| h IH]; intros [entries | keys children] well; simpl in well;
    try contradiction.
  - destruct well as [enough _]. simpl. lia.
  - destruct well as [_ [_ [enough children_wf]]].
    assert (bounded : Forall (fun child => 5 * pow8 h <= size child) children).
    { eapply Forall_impl; [| exact children_wf].
      intros child child_wf. exact (IH child child_wf). }
    pose proof (sum_lower_bound children _ bounded).
    simpl. nia.
Qed.

Theorem ord_root_lower_bound : forall h t, wf true (S h) t -> 10 * pow8 h <= size t.
Proof.
  intros h [entries | keys children] well; simpl in well; [contradiction |].
  destruct well as [_ [_ [enough children_wf]]].
  assert (bounded : Forall (fun child => 5 * pow8 h <= size child) children).
  { eapply Forall_impl; [| exact children_wf].
    intros child child_wf. exact (ord_size_lower_bound h child child_wf). }
  pose proof (sum_lower_bound children _ bounded).
  simpl. nia.
Qed.

(* ------------------------------------------------------------------------ *)
(* Binary search in one node                                                *)

(* imbl's slice_ext::binary_search_by (btree.rs:1300-1350): the comparisons
   that a search between [low] and [high] makes. [order mid] compares the
   element at [mid] with the target. *)
Fixpoint bsearch (fuel low high : nat) (order : nat -> comparison) : nat :=
  match fuel with
  | 0 => 0
  | S fuel' =>
      if low <? high then
        let mid := low + (high - low) / 2 in
        match order mid with
        | Eq => 1
        | Lt => S (bsearch fuel' (S mid) high order)
        | Gt => S (bsearch fuel' low mid order)
        end
      else 0
  end.

Lemma bsearch_le : forall fuel low high order k,
  high - low < 2 ^ k -> bsearch fuel low high order <= k.
Proof.
  induction fuel as [| fuel IH]; intros low high order k range; cbn [bsearch]; [lia |].
  destruct (Nat.ltb_spec low high) as [nonempty | empty]; [| lia].
  destruct k as [| k]; [cbn in range; lia |].
  pose proof (Nat.div_mod (high - low) 2 ltac:(lia)) as split.
  pose proof (Nat.mod_upper_bound (high - low) 2 ltac:(lia)) as remainder.
  set (half := (high - low) / 2) in *.
  cbn [Nat.pow] in range.
  destruct (order (low + half)).
  - lia.
  - apply le_n_S. apply IH. lia.
  - apply le_n_S. apply IH. lia.
Qed.

Theorem bsearch_le_5 : forall fuel n order, n <= 16 -> bsearch fuel 0 n order <= 5.
Proof. intros fuel n order bounded. apply bsearch_le. cbn. lia. Qed.

(* ------------------------------------------------------------------------ *)
(* Searches and the level bound                                             *)

Section Search.

(* The comparisons of the binary search in a node with these keys, and the
   child that the search descends into. bsearch_le_5 discharges the
   hypothesis for imbl's search. *)
Variable comparisons : list nat -> nat.
Variable position : list nat -> nat.
Hypothesis comparisons_le_5 : forall keys, length keys <= 16 -> comparisons keys <= 5.

Fixpoint search_cost (fuel : nat) (t : bptree) : nat :=
  match fuel, t with
  | 0, _ => 0
  | S _, BLeaf entries => comparisons entries
  | S fuel', BBranch keys children =>
      comparisons keys +
        match nth_error children (position keys) with
        | Some child => search_cost fuel' child
        | None => 0
        end
  end.

Theorem ord_search_comparisons : forall h root t fuel,
  wf root h t -> search_cost fuel t <= 5 * S h.
Proof.
  induction h as [| h IH]; intros root [entries | keys children] fuel well;
    simpl in well; try contradiction; destruct fuel as [| fuel]; cbn [search_cost]; try lia.
  - destruct well as [_ upper]. pose proof (comparisons_le_5 entries upper). lia.
  - destruct well as [_ [upper [_ children_wf]]].
    pose proof (comparisons_le_5 keys upper).
    destruct (nth_error children (position keys)) as [child |] eqn:found; [| lia].
    apply nth_error_In in found.
    rewrite Forall_forall in children_wf.
    pose proof (IH false child fuel (children_wf child found)). lia.
Qed.

End Search.

(* The level bound of the Rust charge (ord_levels_bound): one level, plus one
   for every h >= 0 with 10 * 8^h <= entries. *)
Fixpoint levels_search (remaining entries height : nat) : nat :=
  match remaining with
  | 0 => height
  | S remaining' =>
      if Nat.leb (10 * pow8 height) entries then levels_search remaining' entries (S height)
      else height
  end.

Definition levels_bound (entries : nat) : nat := S (levels_search entries entries 0).

Lemma levels_search_start : forall remaining entries height,
  height <= levels_search remaining entries height.
Proof.
  induction remaining as [| remaining IH]; intros entries height; cbn [levels_search]; [lia |].
  destruct (Nat.leb (10 * pow8 height) entries); [specialize (IH entries (S height)) |]; lia.
Qed.

Lemma levels_search_fuel : forall remaining entries height,
  levels_search remaining entries height <= height + remaining.
Proof.
  induction remaining as [| remaining IH]; intros entries height; cbn [levels_search]; [lia |].
  destruct (Nat.leb (10 * pow8 height) entries); [specialize (IH entries (S height)) |]; lia.
Qed.

Lemma levels_search_sound : forall remaining entries height j,
  height <= j < levels_search remaining entries height -> 10 * pow8 j <= entries.
Proof.
  induction remaining as [| remaining IH]; intros entries height j range;
    cbn [levels_search] in range; [lia |].
  destruct (Nat.leb (10 * pow8 height) entries) eqn:fits; [| lia].
  apply Nat.leb_le in fits.
  destruct (Nat.eq_dec j height) as [same | other]; [subst; exact fits |].
  apply (IH entries (S height) j). lia.
Qed.

Lemma levels_search_reaches : forall remaining entries height target,
  height <= target -> (forall j, height <= j < target -> 10 * pow8 j <= entries) ->
  target - height <= remaining -> target <= levels_search remaining entries height.
Proof.
  induction remaining as [| remaining IH]; intros entries height target above fits budget;
    cbn [levels_search]; [lia |].
  destruct (Nat.eq_dec height target) as [same | different].
  - subst target.
    destruct (Nat.leb (10 * pow8 height) entries); [| lia].
    pose proof (levels_search_start remaining entries (S height)). lia.
  - assert (lower : 10 * pow8 height <= entries) by (apply fits; lia).
    apply Nat.leb_le in lower. rewrite lower.
    apply IH; [lia | intros j range; apply fits; lia | lia].
Qed.

Theorem ord_levels_bound_monotone : forall a b, a <= b -> levels_bound a <= levels_bound b.
Proof.
  intros a b ab. unfold levels_bound. apply le_n_S.
  apply levels_search_reaches; [lia | |].
  - intros j range. pose proof (levels_search_sound a a 0 j range). lia.
  - pose proof (levels_search_fuel a a 0). lia.
Qed.

(* A well-formed tree with at most n entries has at most levels_bound n
   levels (its height plus one). *)
Theorem ord_levels_within_bound : forall h t n,
  wf true h t -> size t <= n -> S h <= levels_bound n.
Proof.
  intros h t n well sized.
  destruct h as [| h]; [unfold levels_bound; lia |].
  pose proof (ord_root_lower_bound h t well) as fits.
  unfold levels_bound. apply le_n_S.
  apply levels_search_reaches; [lia | |].
  - intros j range. pose proof (pow8_monotone j h ltac:(lia)). lia.
  - pose proof (pow8_grows h). lia.
Qed.

Theorem ord_search_within_bound : forall comparisons position,
  (forall keys, length keys <= 16 -> comparisons keys <= 5) ->
  forall t h n fuel, wf true h t -> size t <= n ->
  search_cost comparisons position fuel t <= 5 * levels_bound n.
Proof.
  intros comparisons position bounded t h n fuel well sized.
  pose proof (ord_search_comparisons comparisons position bounded h true t fuel well).
  pose proof (ord_levels_within_bound h t n well sized). lia.
Qed.

(* The store's key bound, 2^20 keys, gives 7 levels. *)
Example key_bound_levels : levels_bound (2 ^ 20) = 7.
Proof. vm_compute. reflexivity. Qed.

(* ------------------------------------------------------------------------ *)
(* Allocations of a write                                                   *)

(* One level of a write path: a path copy when the node is shared
   (Arc::make_mut), and a new sibling when the node splits. *)
Record level_step := { shared : bool; splits : bool }.

Definition step_allocations (step : level_step) : nat :=
  (if shared step then 1 else 0) + (if splits step then 1 else 0).

(* The nodes that an insert allocates: the steps of its path, plus the
   transient default leaf and the new root of a root split (imbl
   ord/map.rs:704-707: mem::take allocates Node::default). *)
Definition insert_allocations (path : list level_step) (root_split : bool) : nat :=
  fold_right (fun step total => step_allocations step + total) 0 path
  + (if root_split then 2 else 0).

(* The nodes that a replace of a present key allocates: path copies only,
   because a replace never splits (btree.rs:532-535). *)
Definition replace_allocations (path : list level_step) : nat :=
  fold_right (fun step total => (if shared step then 1 else 0) + total) 0 path.

Lemma path_allocations_le : forall path,
  fold_right (fun step total => step_allocations step + total) 0 path <= 2 * length path.
Proof.
  induction path as [| step rest IH]; cbn [fold_right length]; [lia |].
  assert (step_allocations step <= 2)
    by (unfold step_allocations; destruct (shared step), (splits step); cbn beta iota; lia).
  lia.
Qed.

Lemma replace_path_le : forall path, replace_allocations path <= length path.
Proof.
  unfold replace_allocations.
  induction path as [| step rest IH]; cbn [fold_right length]; [lia |].
  assert ((if shared step then 1 else 0) <= 1) by (destruct (shared step); lia).
  lia.
Qed.

Theorem ord_insert_allocations_le : forall t h n path root_split,
  wf true h t -> size t <= n -> length path = S h ->
  insert_allocations path root_split <= 2 * levels_bound n + 2.
Proof.
  intros t h n path root_split well sized length_path.
  pose proof (ord_levels_within_bound h t n well sized).
  pose proof (path_allocations_le path).
  unfold insert_allocations. destruct root_split; lia.
Qed.

Theorem ord_replace_allocations_le : forall t h n path,
  wf true h t -> size t <= n -> length path = S h ->
  replace_allocations path <= levels_bound n.
Proof.
  intros t h n path well sized length_path.
  pose proof (ord_levels_within_bound h t n well sized).
  pose proof (replace_path_le path). lia.
Qed.

(* ------------------------------------------------------------------------ *)
(* Schedule independence                                                    *)

Section Schedule.

Variable op : Type.
Variable key_of : op -> nat.

(* The charge of an operation, given the earlier operations on its own key.
   The index charges depend on no other key: no population, no other key's
   value and no sharing state. *)
Variable charge : list op -> op -> nat.

Definition own (k : nat) (s : list op) : list op := filter (fun o => Nat.eqb (key_of o) k) s.

Fixpoint key_total (seen s : list op) : nat :=
  match s with
  | [] => 0
  | o :: rest => charge seen o + key_total (seen ++ [o]) rest
  end.

Fixpoint run (s : list op) (history : nat -> list op) : nat :=
  match s with
  | [] => 0
  | o :: rest =>
      charge (history (key_of o)) o +
        run rest (fun k => if Nat.eqb k (key_of o) then history k ++ [o] else history k)
  end.

Definition sum_keys (f : nat -> nat) (keys : list nat) : nat :=
  fold_right (fun k total => f k + total) 0 keys.

Lemma sum_keys_cons : forall f k rest, sum_keys f (k :: rest) = f k + sum_keys f rest.
Proof. reflexivity. Qed.

Lemma sum_keys_ext : forall f g keys,
  (forall k, In k keys -> f k = g k) -> sum_keys f keys = sum_keys g keys.
Proof.
  intros f g keys same. induction keys as [| k rest IH]; [reflexivity |].
  rewrite !sum_keys_cons, (same k (or_introl eq_refl)), IH; [reflexivity |].
  intros k' found. apply same. right. exact found.
Qed.

Lemma sum_keys_zero : forall f keys, (forall k, In k keys -> f k = 0) -> sum_keys f keys = 0.
Proof.
  intros f keys zero. induction keys as [| k rest IH]; [reflexivity |].
  rewrite sum_keys_cons, (zero k (or_introl eq_refl)), IH; [reflexivity |].
  intros k' found. apply zero. right. exact found.
Qed.

(* One key of a duplicate-free list gains [extra]; every other key keeps its
   value. *)
Lemma sum_keys_one : forall f g keys target extra,
  NoDup keys -> In target keys ->
  f target = extra + g target -> (forall k, k <> target -> f k = g k) ->
  sum_keys f keys = extra + sum_keys g keys.
Proof.
  intros f g keys target extra unique found at_target elsewhere.
  induction unique as [| k rest absent unique IH]; [destruct found |].
  rewrite !sum_keys_cons. destruct found as [same | later].
  - subst k.
    rewrite (sum_keys_ext f g rest); [lia |].
    intros k' found'. apply elsewhere. intros equal. subst k'. contradiction.
  - rewrite (IH later).
    assert (different : k <> target) by (intros equal; subst k; contradiction).
    rewrite (elsewhere k different). lia.
Qed.

Lemma run_decomposes : forall s keys history,
  NoDup keys -> (forall o, In o s -> In (key_of o) keys) ->
  run s history = sum_keys (fun k => key_total (history k) (own k s)) keys.
Proof.
  induction s as [| o rest IH]; intros keys history unique covered; cbn [run].
  - symmetry. apply sum_keys_zero. intros k _. reflexivity.
  - rewrite (IH keys); [| exact unique | intros o' found; apply covered; right; exact found].
    symmetry.
    apply (sum_keys_one _ _ keys (key_of o) (charge (history (key_of o)) o) unique
             (covered o (or_introl eq_refl))).
    + unfold own at 1. cbn [filter]. rewrite Nat.eqb_refl. cbn [key_total].
      reflexivity.
    + intros k different. unfold own at 1. cbn [filter].
      destruct (Nat.eqb_spec (key_of o) k) as [equal | _]; [congruence |].
      destruct (Nat.eqb_spec k (key_of o)) as [equal | _]; [congruence |].
      reflexivity.
Qed.

Theorem per_key_schedule_total_invariant : forall s1 s2 keys,
  NoDup keys ->
  (forall o, In o s1 -> In (key_of o) keys) ->
  (forall o, In o s2 -> In (key_of o) keys) ->
  (forall k, own k s1 = own k s2) ->
  run s1 (fun _ => []) = run s2 (fun _ => []).
Proof.
  intros s1 s2 keys unique covered1 covered2 same.
  rewrite (run_decomposes s1 keys _ unique covered1), (run_decomposes s2 keys _ unique covered2).
  apply sum_keys_ext. intros k _. rewrite same. reflexivity.
Qed.

End Schedule.

(* ------------------------------------------------------------------------ *)
(* Negative control and contrast                                            *)

Inductive population_op := Insert (k : nat) | Read (k : nat).

Definition population_key (o : population_op) : nat :=
  match o with Insert k | Read k => k end.

(* The legacy charge: one plus the population of the shard that the keys
   share. *)
Fixpoint run_population (present : list nat) (s : list population_op) : nat :=
  match s with
  | [] => 0
  | o :: rest =>
      S (length present) +
        run_population (match o with Insert k => k :: present | Read _ => present end) rest
  end.

(* Negative control: two schedules with the same order of each key's
   operations have different totals under the population charge (5 and 6). *)
Example population_charge_schedule_dependent :
  (forall k, filter (fun o => Nat.eqb (population_key o) k) [Insert 1; Read 1; Insert 2] =
             filter (fun o => Nat.eqb (population_key o) k) [Insert 1; Insert 2; Read 1]) /\
  run_population [] [Insert 1; Read 1; Insert 2] <> run_population [] [Insert 1; Insert 2; Read 1].
Proof.
  split; [| cbn; lia].
  intros k. cbn [filter population_key].
  destruct (Nat.eqb_spec 1 k), (Nat.eqb_spec 2 k); try reflexivity; lia.
Qed.

(* Contrast: the legacy insert model (collection_backing.rs,
   persistent_insert_backing) charged 12 * min(n + 1, 33) + 2 nodes, 398 for
   a shard of 32 entries; the index allocates at most 2 * 7 + 2 = 16. *)
Definition legacy_insert_nodes (entries : nat) : nat :=
  if Nat.eqb entries 0 then 1 else 12 * Nat.min (S entries) 33 + 2.

Example legacy_insert_nodes_example :
  legacy_insert_nodes 32 = 398 /\ 2 * levels_bound (2 ^ 20) + 2 = 16.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions ord_size_lower_bound.
Print Assumptions ord_root_lower_bound.
Print Assumptions bsearch_le_5.
Print Assumptions ord_search_comparisons.
Print Assumptions ord_levels_bound_monotone.
Print Assumptions ord_levels_within_bound.
Print Assumptions ord_search_within_bound.
Print Assumptions key_bound_levels.
Print Assumptions ord_insert_allocations_le.
Print Assumptions ord_replace_allocations_le.
Print Assumptions per_key_schedule_total_invariant.
Print Assumptions population_charge_schedule_dependent.
Print Assumptions legacy_insert_nodes_example.
