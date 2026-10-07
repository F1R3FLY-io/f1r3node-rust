(* C3 (epic 8946, B1 Phase A; decision record DR-78): a B-tree lookup is
   charged by the search bound of the tree's size, not linearly.

   The standard library's BTreeMap uses B = 6: a node holds at most 11 keys,
   and every node except the root holds at least 5. An internal node with k
   keys has k + 1 children, and all leaves have the same depth. A search
   scans the keys of one node from the left until it finds a key that is not
   smaller than the target, then either stops or descends into one child.

   Results:
   - btree_size_lower_bound: a well-formed subtree of height h holds at least
     6^h - 1 entries, and a well-formed root of height h + 1 holds at least
     2 * 6^h - 1 entries.
   - btree_search_comparisons: a search in a well-formed tree of height h
     makes at most 11 * h key comparisons.
   - btree_height_bound and search_within_size_bound: the height, and so
     the comparisons, are bounded by a function of the size alone
     (height_bound, the bound that the Rust charge uses).
   - linear_charge_example: the legacy linear charge (one comparison per
     entry) exceeds the bound by orders of magnitude.

   C14 (decision record DR-79) charges replay authority tree visits with the
   same bound and adds:
   - height_bound_monotone: a larger size never has a smaller height bound.
   - search_visits_le_height: a search reads at most h nodes.
   - search_within_bound: a search in a map with at most n entries makes at
     most 11 * height_bound n comparisons and reads at most height_bound n
     nodes. A charge is sound if its size bounds the map when the search
     runs.
   - pre_operation_size_undercharges and summed_size_undercharges: two
     negative controls. A deferred search charged at the size before the
     operation, and one bound for the summed size of two maps, both charge
     less than the real search.

   Rust correspondence: shared/src/rust/collection_backing.rs
   (tree_height_bound, tree_search_bound); its users are the produce-counter
   lookups in rspace++/src/rspace/replay_rspace/native_candidate/metered.rs
   (prepare_metered_produce_counter, metered_produce_count, metered_comm) and
   the replay authority charges in rholang/src/rust/interpreter/accounting/
   native_runtime/replay_authority/backing.rs (tree_update,
   reserve_event_lookup, reserve_changes). The extracted property tests count
   the comparisons of the real BTreeMap with a counting key type. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive btree := Node (keys : list nat) (children : list btree).

Fixpoint size (t : btree) : nat :=
  match t with
  | Node keys children => length keys + fold_right Nat.add 0 (map size children)
  end.

(* wf root h t: t is a B-tree of height h; [root] relaxes the lower bound on
   the number of keys of the top node to one. *)
Fixpoint wf (root : bool) (h : nat) (t : btree) {struct h} : Prop :=
  match h, t with
  | 0, _ => False
  | S lower, Node keys children =>
      (if root then 1 else 5) <= length keys /\ length keys <= 11 /\
      match lower with
      | 0 => children = []
      | S _ => length children = S (length keys) /\ Forall (wf false lower) children
      end
  end.

Fixpoint pow6 (h : nat) : nat := match h with 0 => 1 | S lower => 6 * pow6 lower end.

Lemma pow6_positive : forall h, 1 <= pow6 h.
Proof. induction h; simpl; lia. Qed.

Lemma sum_lower_bound : forall (children : list btree) minimum,
  Forall (fun child => minimum <= size child) children ->
  length children * minimum <= fold_right Nat.add 0 (map size children).
Proof.
  intros children minimum bounded.
  induction bounded as [| child rest child_bound _ IH]; simpl; lia.
Qed.

Lemma wf_subtree_lower_bound : forall h t,
  wf false (S h) t -> pow6 (S h) - 1 <= size t.
Proof.
  induction h as [| h IH]; intros [keys children] well.
  - (* non-root leaf: at least five keys *)
    destruct well as [enough [_ leaf]]. subst children. simpl. lia.
  - (* non-root internal node: at least five keys and six subtrees *)
    destruct well as [enough [_ [count children_wf]]].
    assert (bounded : Forall (fun child => pow6 (S h) - 1 <= size child) children).
    { eapply Forall_impl; [| exact children_wf].
      intros child child_wf. exact (IH child child_wf). }
    pose proof (sum_lower_bound children _ bounded).
    pose proof (pow6_positive h).
    simpl in *. nia.
Qed.

Lemma wf_root_lower_bound : forall h t,
  wf true (S (S h)) t -> 2 * pow6 (S h) - 1 <= size t.
Proof.
  intros h [keys children] well.
  destruct well as [enough [_ [count children_wf]]].
  assert (bounded : Forall (fun child => pow6 (S h) - 1 <= size child) children).
  { eapply Forall_impl; [| exact children_wf].
    intros child child_wf. exact (wf_subtree_lower_bound h child child_wf). }
  pose proof (sum_lower_bound children _ bounded).
  pose proof (pow6_positive h).
  simpl in *. nia.
Qed.

Theorem btree_size_lower_bound : forall h t,
  (wf false (S h) t -> pow6 (S h) - 1 <= size t) /\
  (wf true (S (S h)) t -> 2 * pow6 (S h) - 1 <= size t).
Proof.
  intros h t. split; [apply wf_subtree_lower_bound | apply wf_root_lower_bound].
Qed.

(* A linear-scan search: comparisons in this node, then at most one child. *)
Fixpoint scan (keys : list nat) (target : nat) : nat * nat :=
  match keys with
  | [] => (0, 0)
  | key :: rest =>
      if Nat.leb target key then (1, 0)
      else let (comparisons, position) := scan rest target in (S comparisons, S position)
  end.

Lemma scan_bounds : forall keys target,
  fst (scan keys target) <= length keys /\ snd (scan keys target) <= length keys.
Proof.
  induction keys as [| key rest IH]; intros target; simpl; [lia |].
  destruct (Nat.leb target key); simpl; [lia |].
  destruct (scan rest target) as [comparisons position] eqn:scanned.
  specialize (IH target). rewrite scanned in IH. simpl in *. lia.
Qed.

Fixpoint search_comparisons (fuel : nat) (t : btree) (target : nat) : nat :=
  match fuel, t with
  | 0, _ => 0
  | S fuel, Node keys children =>
      let (comparisons, position) := scan keys target in
      comparisons +
        match nth_error children position with
        | Some child => search_comparisons fuel child target
        | None => 0
        end
  end.

Theorem btree_search_comparisons : forall h root t target fuel,
  wf root h t -> search_comparisons fuel t target <= 11 * h.
Proof.
  induction h as [| h IH]; intros root [keys children] target fuel well; [contradiction |].
  destruct fuel as [| fuel]; simpl; [lia |].
  destruct well as [_ [upper rest]].
  pose proof (scan_bounds keys target) as [comparisons_bound position_bound].
  destruct (scan keys target) as [comparisons position] eqn:scanned. simpl in *.
  destruct h as [| lower].
  - subst children. destruct position; simpl; lia.
  - destruct rest as [_ children_wf].
    destruct (nth_error children position) as [child |] eqn:found; [| lia].
    apply nth_error_In in found.
    rewrite Forall_forall in children_wf.
    pose proof (IH false child target fuel (children_wf child found)). lia.
Qed.

(* The height bound that the Rust charge computes: the largest height whose
   minimum root size does not exceed the size. *)
Fixpoint height_search (remaining entries height : nat) : nat :=
  match remaining with
  | 0 => height
  | S remaining =>
      if Nat.leb (2 * pow6 height - 1) entries
      then height_search remaining entries (S height)
      else height
  end.

Definition height_bound (entries : nat) : nat :=
  match entries with 0 => 0 | S _ => height_search entries entries 1 end.

Lemma root_minimum_monotone : forall a b, a <= b -> 2 * pow6 a - 1 <= 2 * pow6 b - 1.
Proof.
  intros a b ab. induction ab as [| b ab IH]; [lia |].
  simpl. pose proof (pow6_positive b). lia.
Qed.

Lemma pow6_grows : forall h, h < 2 * pow6 h - 1 \/ h = 0.
Proof.
  induction h as [| h IH]; [right; reflexivity |].
  left. simpl. destruct IH as [IH | IH]; [lia |]. subst. simpl. lia.
Qed.

Lemma height_search_reaches : forall remaining entries height target,
  1 <= height -> height <= target -> 2 * pow6 (target - 1) - 1 <= entries ->
  target - height <= remaining ->
  target <= height_search remaining entries height.
Proof.
  induction remaining as [| remaining IH]; intros entries height target positive above fits budget;
    cbn [height_search]; [lia |].
  destruct (Nat.eqb height target) eqn:same.
  - apply Nat.eqb_eq in same. subst target.
    destruct (Nat.leb (2 * pow6 height - 1) entries); [| lia].
    assert (forall r e h, h <= height_search r e h) as grows.
    { induction r as [| r IHr]; intros e h0; cbn [height_search]; [lia |].
      destruct (Nat.leb (2 * pow6 h0 - 1) e); [specialize (IHr e (S h0)); lia | lia]. }
    specialize (grows remaining entries (S height)). lia.
  - apply Nat.eqb_neq in same.
    assert (lower : 2 * pow6 height - 1 <= entries).
    { pose proof (root_minimum_monotone height (target - 1) ltac:(lia)). lia. }
    apply Nat.leb_le in lower. rewrite lower.
    apply IH; lia.
Qed.

(* A well-formed root of height h with n entries satisfies h <= height_bound n. *)
Theorem btree_height_bound : forall h t,
  wf true h t -> h <= height_bound (size t).
Proof.
  intros h t well.
  destruct h as [| h]; [contradiction |].
  assert (fits : 2 * pow6 h - 1 <= size t).
  { destruct h as [| h].
    - destruct t as [keys children]. destruct well as [enough _]. simpl. lia.
    - exact (proj2 (btree_size_lower_bound h t) well). }
  unfold height_bound.
  destruct (size t) as [| entries] eqn:sized.
  - pose proof (pow6_positive h). lia.
  - apply height_search_reaches; try lia.
    + replace (S h - 1) with h by lia. exact fits.
    + destruct (pow6_grows h) as [grows | zero]; lia.
Qed.

Theorem search_within_size_bound : forall t target fuel h,
  wf true h t -> search_comparisons fuel t target <= 11 * height_bound (size t).
Proof.
  intros t target fuel h well.
  pose proof (btree_search_comparisons h true t target fuel well).
  pose proof (btree_height_bound h t well). lia.
Qed.

(* For 2,000 entries the legacy linear charge counts 2,001 comparisons; the
   bound is 11 * 4 = 44. *)
Example linear_charge_example : height_bound 2000 = 4.
Proof. vm_compute. reflexivity. Qed.

(* C14 (decision record DR-79): the replay authority charges each B-tree
   visit by the height bound of a size that holds when the visit runs. The
   results below extend the C3 model for that use. *)

Lemma height_search_lower : forall remaining entries height,
  2 * pow6 (height - 1) - 1 <= entries ->
  2 * pow6 (height_search remaining entries height - 1) - 1 <= entries.
Proof.
  induction remaining as [| remaining IH]; intros entries height fits;
    cbn [height_search]; [exact fits |].
  destruct (Nat.leb (2 * pow6 height - 1) entries) eqn:step; [| exact fits].
  apply IH. apply Nat.leb_le in step. replace (S height - 1) with height by lia.
  exact step.
Qed.

Lemma height_search_upper : forall remaining entries height,
  1 <= height -> entries < height + remaining ->
  entries < 2 * pow6 (height_search remaining entries height) - 1.
Proof.
  induction remaining as [| remaining IH]; intros entries height positive fuel;
    cbn [height_search].
  - destruct (pow6_grows height) as [grows | zero]; lia.
  - destruct (Nat.leb (2 * pow6 height - 1) entries) eqn:step.
    + apply IH; lia.
    + apply Nat.leb_gt in step. exact step.
Qed.

Lemma height_bound_upper : forall entries, entries < 2 * pow6 (height_bound entries) - 1.
Proof.
  intros [| entries]; unfold height_bound; [simpl; lia |].
  apply height_search_upper; lia.
Qed.

Lemma height_bound_lower : forall entries, 1 <= entries ->
  2 * pow6 (height_bound entries - 1) - 1 <= entries.
Proof.
  intros [| entries] positive; [lia |]. unfold height_bound.
  apply height_search_lower. simpl. lia.
Qed.

(* A larger size never has a smaller height bound. *)
Theorem height_bound_monotone : forall a b, a <= b -> height_bound a <= height_bound b.
Proof.
  intros a b ab.
  destruct a as [| a]; [apply Nat.le_0_l |].
  destruct (Nat.le_gt_cases (height_bound (S a)) (height_bound b)) as [ok | wrong];
    [exact ok |].
  exfalso.
  pose proof (height_bound_lower (S a) ltac:(lia)) as lower.
  pose proof (height_bound_upper b) as upper.
  pose proof (root_minimum_monotone (height_bound b) (height_bound (S a) - 1) ltac:(lia))
    as mono.
  lia.
Qed.

(* The nodes that one search reads: this node, then at most one child. *)
Fixpoint search_visits (fuel : nat) (t : btree) (target : nat) : nat :=
  match fuel, t with
  | 0, _ => 0
  | S fuel, Node keys children =>
      let (_, position) := scan keys target in
      S match nth_error children position with
        | Some child => search_visits fuel child target
        | None => 0
        end
  end.

Theorem search_visits_le_height : forall h root t target fuel,
  wf root h t -> search_visits fuel t target <= h.
Proof.
  induction h as [| h IH]; intros root [keys children] target fuel well; [contradiction |].
  destruct fuel as [| fuel]; simpl; [lia |].
  destruct well as [_ [_ rest]].
  destruct (scan keys target) as [comparisons position] eqn:scanned. simpl in *.
  destruct h as [| lower].
  - subst children. destruct position; simpl; lia.
  - destruct rest as [_ children_wf].
    destruct (nth_error children position) as [child |] eqn:found; [| lia].
    apply nth_error_In in found.
    rewrite Forall_forall in children_wf.
    pose proof (IH false child target fuel (children_wf child found)). lia.
Qed.

(* A search in a map with at most [entries] entries makes at most
   11 * height_bound entries comparisons and reads at most height_bound
   entries nodes. This is the charge of one replay authority visit. *)
Theorem search_within_bound : forall t target fuel h entries,
  wf true h t -> size t <= entries ->
  search_comparisons fuel t target <= 11 * height_bound entries /\
  search_visits fuel t target <= height_bound entries.
Proof.
  intros t target fuel h entries well sized.
  pose proof (btree_search_comparisons h true t target fuel well).
  pose proof (search_visits_le_height h true t target fuel well).
  pose proof (btree_height_bound h t well).
  pose proof (height_bound_monotone (size t) entries sized).
  split; lia.
Qed.

(* Negative control 1: a deferred search charged at the size before the
   operation. The map is empty when the charge is computed, so the bound is
   0, but it holds one entry when the search runs. *)
Example pre_operation_size_undercharges :
  wf true 1 (Node [5] []) /\ size (Node [5] []) = 1 /\
  11 * height_bound 0 < search_comparisons 1 (Node [5] []) 5.
Proof. split; [simpl; repeat split; lia |]. split; reflexivity || (simpl; lia). Qed.

(* Negative control 2: one bound for the summed size of two maps. Each map
   is a well-formed tree of height 2 with 77 entries, and a search for a key
   above every stored key makes 22 comparisons in each. The bound of the
   summed size, 154 entries, allows only 33. *)
Definition full_leaf : btree := Node (repeat 0 11) [].
Definition minimal_leaf : btree := Node (repeat 0 5) [].
Definition dense_tree : btree := Node (repeat 0 11) (repeat minimal_leaf 11 ++ [full_leaf]).

Example summed_size_undercharges :
  wf true 2 dense_tree /\ size dense_tree = 77 /\
  search_comparisons 2 dense_tree 1 = 22 /\
  11 * height_bound (size dense_tree + size dense_tree) <
    search_comparisons 2 dense_tree 1 + search_comparisons 2 dense_tree 1.
Proof.
  split.
  - simpl. repeat split; try lia. repeat constructor; simpl; lia.
  - split; [reflexivity |]. split; [reflexivity |]. vm_compute. lia.
Qed.

Print Assumptions btree_size_lower_bound.
Print Assumptions btree_search_comparisons.
Print Assumptions btree_height_bound.
Print Assumptions search_within_size_bound.
Print Assumptions linear_charge_example.
Print Assumptions height_bound_monotone.
Print Assumptions search_visits_le_height.
Print Assumptions search_within_bound.
Print Assumptions pre_operation_size_undercharges.
Print Assumptions summed_size_undercharges.
