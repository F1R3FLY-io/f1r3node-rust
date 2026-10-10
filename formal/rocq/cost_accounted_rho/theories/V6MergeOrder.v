(** * V6MergeOrder

    DR-120 (gap G9): the order K of the ordered pass (order.rs).

    The ordered pass sorts its candidates by K and walks them once
    (ordered.rs). K compares two candidates by these keys, in this order:
    1. not pinned: a candidate that holds a settled chain comes first (#341);
    2. the highest prior-loss count, descending (#294);
    3. the saturating sum of the prior-loss counts, descending;
    4. the lowest source height, ascending;
    5. dev's [compare_branches]: the length, then the sorted chains, one
       pair at a time (conflict_set_merger.rs:38-80).

    A chain is modelled by its rank in the total order of [DeployChainIndex]
    (deploy_chain_index.rs:179-259). That order is strict and total, and its
    [Equal] coincides with the chain equality of deploy_chain_index.rs:163-171.
    So the chains of one merge have distinct ranks. Every chain set of one
    merge is a [HashableSet] under that equality, so the pinned flag, the
    loss count and the source height are functions of the rank.

    Main results:
    - [k_eq_iff_same_branch]: K returns [Eq] exactly for two lists of the same
      chains, in any order.
    - [k_strict_total]: K is antisymmetric, transitive and total on chain
      sets, so it is a strict total order.
    - [ordered_pass_perm_invariant]: the sorted candidate list, and so every
      walk over it, does not depend on the arrival order.
    - [nc_position_tiebreak_not_perm_invariant]: with the arrival position as
      the last tie-break, the sorted list depends on the arrival order. *)

From Stdlib Require Import ZArith Lists.List Lia Bool Arith.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

(** ** Insertion sort by a comparison function *)

Section InsertionSort.

Variable A : Type.
Variable cmp : A -> A -> comparison.

(** [insert x l] puts [x] before the first element that is not smaller.
    Ties keep [x] first, so a sort by a key alone keeps the arrival order of
    equal keys. *)
Fixpoint insert (x : A) (l : list A) : list A :=
  match l with
  | [] => [x]
  | y :: rest =>
      match cmp x y with
      | Gt => y :: insert x rest
      | _ => x :: y :: rest
      end
  end.

Fixpoint isort (l : list A) : list A :=
  match l with
  | [] => []
  | x :: rest => insert x (isort rest)
  end.

Lemma insert_perm : forall x l, Permutation (x :: l) (insert x l).
Proof.
  intros x l. induction l as [| y rest IH]; simpl; [reflexivity |].
  destruct (cmp x y); try reflexivity.
  transitivity (y :: x :: rest); [apply perm_swap | apply perm_skip; exact IH].
Qed.

Lemma isort_perm : forall l, Permutation l (isort l).
Proof.
  induction l as [| x rest IH]; simpl; [reflexivity |].
  transitivity (x :: isort rest); [apply perm_skip; exact IH | apply insert_perm].
Qed.

(** [le_by a b]: [a] may come before [b]. *)
Definition le_by (a b : A) : Prop := cmp a b <> Gt.

Hypothesis cmp_antisym : forall a b, cmp b a = CompOpp (cmp a b).
Hypothesis cmp_eq_subst : forall a b c, cmp a b = Eq -> cmp a c = cmp b c.
Hypothesis cmp_lt_trans : forall a b c, cmp a b = Lt -> cmp b c = Lt -> cmp a c = Lt.

Lemma insert_hd :
  forall y x l, le_by y x -> HdRel le_by y l -> HdRel le_by y (insert x l).
Proof.
  intros y x l Hyx Hhd. destruct l as [| z rest]; simpl.
  - constructor. exact Hyx.
  - destruct (cmp x z); constructor; try exact Hyx.
    inversion Hhd; assumption.
Qed.

Lemma insert_sorted : forall x l, Sorted le_by l -> Sorted le_by (insert x l).
Proof.
  intros x l Hs. induction Hs as [| y rest Hrest IH Hhd]; simpl.
  - repeat constructor.
  - destruct (cmp x y) eqn:Hxy.
    + constructor; [constructor; assumption |].
      constructor. unfold le_by. rewrite Hxy. discriminate.
    + constructor; [constructor; assumption |].
      constructor. unfold le_by. rewrite Hxy. discriminate.
    + constructor; [exact IH |]. apply insert_hd; [| exact Hhd].
      unfold le_by. rewrite cmp_antisym, Hxy. simpl. discriminate.
Qed.

Lemma isort_sorted : forall l, Sorted le_by (isort l).
Proof.
  induction l as [| x rest IH]; simpl; [constructor | apply insert_sorted; exact IH].
Qed.

Lemma le_by_trans : forall a b c, le_by a b -> le_by b c -> le_by a c.
Proof.
  unfold le_by. intros a b c Hab Hbc.
  destruct (cmp a b) eqn:Eab; [| | exfalso; apply Hab; reflexivity].
  - rewrite (cmp_eq_subst a b c Eab). exact Hbc.
  - destruct (cmp b c) eqn:Ebc; [| | exfalso; apply Hbc; reflexivity].
    + assert (Hcb : cmp c b = Eq) by (rewrite cmp_antisym, Ebc; reflexivity).
      rewrite cmp_antisym, (cmp_eq_subst c b a Hcb), <- cmp_antisym, Eab.
      discriminate.
    + rewrite (cmp_lt_trans a b c Eab Ebc). discriminate.
Qed.

Lemma le_by_antisym : forall a b, le_by a b -> le_by b a -> cmp a b = Eq.
Proof.
  unfold le_by. intros a b Hab Hba. rewrite cmp_antisym in Hba.
  destruct (cmp a b); [reflexivity | exfalso; apply Hba; reflexivity |
                       exfalso; apply Hab; reflexivity].
Qed.

(** Two sorted permutations of each other are equal, when [cmp] returns
    [Eq] only for equal elements of the list. *)
Lemma sorted_perm_unique :
  forall l1 l2,
    Sorted le_by l1 -> Sorted le_by l2 -> Permutation l1 l2 ->
    (forall a b, In a l1 -> In b l1 -> cmp a b = Eq -> a = b) ->
    l1 = l2.
Proof.
  intros l1 l2 Hs1 Hs2.
  apply (Sorted_StronglySorted le_by_trans) in Hs1.
  apply (Sorted_StronglySorted le_by_trans) in Hs2.
  revert l2 Hs2.
  induction Hs1 as [| x r1 Hr1 IH Hx]; intros l2 Hs2 Hperm Hdistinct.
  - symmetry. exact (Permutation_nil Hperm).
  - destruct l2 as [| y r2].
    + exfalso. apply Permutation_sym, Permutation_nil in Hperm. discriminate Hperm.
    + apply StronglySorted_inv in Hs2 as [Hr2 Hy].
      assert (Hxy : x = y).
      { assert (Hx_in : In x (y :: r2)) by (apply (Permutation_in x Hperm); left; reflexivity).
        assert (Hy_in : In y (x :: r1))
          by (apply (Permutation_in y (Permutation_sym Hperm)); left; reflexivity).
        destruct Hx_in as [Heq | Hx_r2]; [symmetry; exact Heq |].
        destruct Hy_in as [Heq | Hy_r1]; [exact Heq |].
        apply Hdistinct; [left; reflexivity | right; exact Hy_r1 |].
        apply le_by_antisym.
        - rewrite Forall_forall in Hx. exact (Hx y Hy_r1).
        - rewrite Forall_forall in Hy. exact (Hy x Hx_r2). }
      subst y. f_equal.
      apply IH; [exact Hr2 | exact (Permutation_cons_inv Hperm) |].
      intros a b Ha Hb Hab. apply Hdistinct; [right; exact Ha | right; exact Hb | exact Hab].
Qed.

Lemma isort_perm_unique :
  forall l1 l2,
    Permutation l1 l2 ->
    (forall a b, In a l1 -> In b l1 -> cmp a b = Eq -> a = b) ->
    isort l1 = isort l2.
Proof.
  intros l1 l2 Hperm Hdistinct.
  apply sorted_perm_unique; [apply isort_sorted | apply isort_sorted | |].
  - transitivity l1; [symmetry; apply isort_perm |].
    transitivity l2; [exact Hperm | apply isort_perm].
  - intros a b Ha Hb. apply Hdistinct.
    + exact (Permutation_in a (Permutation_sym (isort_perm l1)) Ha).
    + exact (Permutation_in b (Permutation_sym (isort_perm l1)) Hb).
Qed.

End InsertionSort.

Arguments insert {A} cmp x l.
Arguments isort {A} cmp l.
Arguments le_by {A} cmp a b.

(** ** Lexicographic comparison of integer lists *)

Fixpoint lexZ (a b : list Z) : comparison :=
  match a, b with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: a', y :: b' => match Z.compare x y with Eq => lexZ a' b' | c => c end
  end.

Lemma lexZ_antisym : forall a b, lexZ b a = CompOpp (lexZ a b).
Proof.
  induction a as [| x a IH]; destruct b as [| y b]; simpl; try reflexivity.
  rewrite (Z.compare_antisym x y).
  destruct (Z.compare x y); simpl; [apply IH | reflexivity | reflexivity].
Qed.

Lemma lexZ_eq : forall a b, lexZ a b = Eq -> a = b.
Proof.
  induction a as [| x a IH]; intros b H; destruct b as [| y b]; simpl in H;
    try discriminate H; [reflexivity |].
  destruct (Z.compare x y) eqn:Hxy; try discriminate H.
  apply Z.compare_eq_iff in Hxy. subst y. f_equal. exact (IH b H).
Qed.

Lemma lexZ_refl : forall a, lexZ a a = Eq.
Proof.
  induction a as [| x a IH]; simpl; [reflexivity |]. rewrite Z.compare_refl. exact IH.
Qed.

Lemma lexZ_trans : forall a b c, lexZ a b = Lt -> lexZ b c = Lt -> lexZ a c = Lt.
Proof.
  induction a as [| x a IH]; intros b c Hab Hbc; destruct b as [| y b]; destruct c as [| z c];
    simpl in *; try discriminate; try reflexivity.
  destruct (Z.compare x y) eqn:Hxy; try discriminate Hab;
    destruct (Z.compare y z) eqn:Hyz; try discriminate Hbc.
  - apply Z.compare_eq_iff in Hxy. apply Z.compare_eq_iff in Hyz. subst y z.
    rewrite Z.compare_refl. exact (IH b c Hab Hbc).
  - apply Z.compare_eq_iff in Hxy. subst y. rewrite Hyz. reflexivity.
  - apply Z.compare_eq_iff in Hyz. subst z. rewrite Hxy. reflexivity.
  - change (x < y)%Z in Hxy. change (y < z)%Z in Hyz.
    assert (Hxz : (x < z)%Z) by lia.
    change ((x ?= z)%Z = Lt) in Hxz. rewrite Hxz. reflexivity.
Qed.

(** ** The order K *)

Lemma map_of_nat_inj : forall a b, map Z.of_nat a = map Z.of_nat b -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H; simpl in H; try discriminate H; [reflexivity |].
  injection H as Hxy Hab. f_equal; [exact (Nat2Z.inj x y Hxy) | exact (IH b Hab)].
Qed.

(** The range of an i64 and of a u64. *)
Definition I64MAX : Z := 9223372036854775807.
Definition U64MAX : Z := 18446744073709551615.

(** [u64::saturating_add] for two values in [0, U64MAX]. *)
Definition sat_add (a b : Z) : Z := Z.min (a + b) U64MAX.

(** [then_cmp c k]: [Ordering::then_with]. *)
Definition then_cmp (c k : comparison) : comparison :=
  match c with Eq => k | _ => c end.

Lemma then_cmp_assoc : forall x y z, then_cmp (then_cmp x y) z = then_cmp x (then_cmp y z).
Proof. intros [] y z; reflexivity. Qed.

(** The order of [bool] in Rust: [false < true]. *)
Definition bool_cmp (x y : bool) : comparison :=
  match x, y with
  | false, true => Lt
  | true, false => Gt
  | _, _ => Eq
  end.

(** dev's element loop (conflict_set_merger.rs:65-72): the pairs of a zip,
    one at a time. *)
Fixpoint zip_cmp (a b : list nat) : comparison :=
  match a, b with
  | x :: a', y :: b' => then_cmp (Nat.compare x y) (zip_cmp a' b')
  | _, _ => Eq
  end.

(** [compare_branches] (conflict_set_merger.rs:38-80): the lengths, then
    the sorted chains. *)
Definition compare_branches (a b : list nat) : comparison :=
  then_cmp (Nat.compare (length a) (length b))
           (zip_cmp (isort Nat.compare a) (isort Nat.compare b)).

Section Order.

(** The settled flag, the prior-loss count ([u64]) and the source height
    ([i64]) of a chain. *)
Variable pinned : nat -> bool.
Variable loss : nat -> Z.
Variable height : nat -> Z.
Hypothesis loss_nonneg : forall c, (0 <= loss c)%Z.

(** order.rs: [is_pinned], and [branch_losses] with [LossProfile::fold]
    (conflict_set_merger.rs:761-786), and the lowest height with
    [unwrap_or(i64::MAX)]. *)
Definition bpinned (b : list nat) : bool := existsb pinned b.
Definition loss_max (b : list nat) : Z := fold_left (fun acc c => Z.max acc (loss c)) b 0%Z.
Definition loss_sum (b : list nat) : Z := fold_left (fun acc c => sat_add acc (loss c)) b 0%Z.
Definition lowest_height (b : list nat) : Z :=
  fold_left (fun acc c => Z.min acc (height c)) b I64MAX.

(** [numeric_key] of order.rs: (not pinned, Reverse(max), Reverse(sum),
    lowest height), compared as a Rust tuple. *)
Definition numeric_cmp (a b : list nat) : comparison :=
  then_cmp (bool_cmp (negb (bpinned a)) (negb (bpinned b)))
    (then_cmp (Z.compare (loss_max b) (loss_max a))
      (then_cmp (Z.compare (loss_sum b) (loss_sum a))
        (Z.compare (lowest_height a) (lowest_height b)))).

(** [k_cmp] of order.rs. *)
Definition k_cmp (a b : list nat) : comparison :=
  then_cmp (numeric_cmp a b) (compare_branches a b).

(** [sort_by_k] of order.rs. *)
Definition sort_by_k (cs : list (list nat)) : list (list nat) := isort k_cmp cs.

(** K as one integer list, compared lexicographically. *)
Definition key (b : list nat) : list Z :=
  [Z.b2z (negb (bpinned b)); Z.opp (loss_max b); Z.opp (loss_sum b); lowest_height b;
   Z.of_nat (length b)] ++ map Z.of_nat (isort Nat.compare b).

Lemma lexZ_cons : forall x y a b,
  lexZ (x :: a) (y :: b) = then_cmp (Z.compare x y) (lexZ a b).
Proof. intros. simpl. destruct (Z.compare x y); reflexivity. Qed.

Lemma lexZ_map_zip : forall a b, length a = length b ->
  lexZ (map Z.of_nat a) (map Z.of_nat b) = zip_cmp a b.
Proof.
  induction a as [| x a IH]; destruct b as [| y b]; simpl; try discriminate; [reflexivity |].
  intros Hlen. injection Hlen as Hlen.
  rewrite Nat2Z.inj_compare. rewrite (IH b Hlen).
  destruct (Nat.compare x y); reflexivity.
Qed.

Lemma isort_length : forall (A : Type) (cmp : A -> A -> comparison) l,
  length (isort cmp l) = length l.
Proof.
  intros A cmp l. symmetry. apply Permutation_length. apply isort_perm.
Qed.

Lemma k_cmp_key : forall a b, k_cmp a b = lexZ (key a) (key b).
Proof.
  intros a b. unfold k_cmp, numeric_cmp, compare_branches, key.
  cbn [app]. rewrite !lexZ_cons, !Z.compare_opp, Nat2Z.inj_compare, !then_cmp_assoc.
  assert (Hpin : Z.compare (Z.b2z (negb (bpinned a))) (Z.b2z (negb (bpinned b)))
                 = bool_cmp (negb (bpinned a)) (negb (bpinned b)))
    by (destruct (negb (bpinned a)), (negb (bpinned b)); reflexivity).
  rewrite Hpin.
  assert (Htail : then_cmp (Nat.compare (length a) (length b))
                    (zip_cmp (isort Nat.compare a) (isort Nat.compare b))
                  = then_cmp (Nat.compare (length a) (length b))
                    (lexZ (map Z.of_nat (isort Nat.compare a))
                          (map Z.of_nat (isort Nat.compare b)))).
  { destruct (Nat.compare (length a) (length b)) eqn:Hlen; [| reflexivity | reflexivity].
    apply Nat.compare_eq_iff in Hlen. simpl.
    symmetry. apply lexZ_map_zip. rewrite !isort_length. exact Hlen. }
  rewrite Htail. reflexivity.
Qed.

(** A fold whose step commutes with itself gives the same result on every
    permutation. *)
Lemma fold_left_perm_swap :
  forall (f : Z -> nat -> Z),
    (forall acc x y, f (f acc x) y = f (f acc y) x) ->
    forall l l', Permutation l l' -> forall acc, fold_left f l acc = fold_left f l' acc.
Proof.
  intros f Hswap l l' H.
  induction H as [| x l l' H IH | x y l | l l' l'' H1 IH1 H2 IH2]; intros acc; simpl.
  - reflexivity.
  - apply IH.
  - rewrite Hswap. reflexivity.
  - rewrite IH1. apply IH2.
Qed.

Lemma existsb_perm : forall (f : nat -> bool) l l',
  Permutation l l' -> existsb f l = existsb f l'.
Proof.
  intros f l l' H. induction H; simpl.
  - reflexivity.
  - rewrite IHPermutation. reflexivity.
  - rewrite !orb_assoc, (orb_comm (f y) (f x)). reflexivity.
  - rewrite IHPermutation1. exact IHPermutation2.
Qed.

Lemma key_perm : forall a b, Permutation a b -> key a = key b.
Proof.
  intros a b H. unfold key, bpinned, loss_max, loss_sum, lowest_height.
  rewrite (existsb_perm pinned a b H).
  rewrite (fold_left_perm_swap (fun acc c => Z.max acc (loss c))
             ltac:(intros acc x y; cbv beta; lia) a b H).
  rewrite (fold_left_perm_swap (fun acc c => sat_add acc (loss c))
             ltac:(intros acc x y; cbv beta; unfold sat_add;
                   pose proof (loss_nonneg x); pose proof (loss_nonneg y); lia) a b H).
  rewrite (fold_left_perm_swap (fun acc c => Z.min acc (height c))
             ltac:(intros acc x y; cbv beta; lia) a b H).
  rewrite (Permutation_length H).
  rewrite (isort_perm_unique nat Nat.compare Nat.compare_antisym
             ltac:(intros x y z Hxy; apply Nat.compare_eq_iff in Hxy; subst; reflexivity)
             ltac:(intros x y z Hxy Hyz; apply Nat.compare_lt_iff in Hxy;
                   apply Nat.compare_lt_iff in Hyz; apply Nat.compare_lt_iff; lia)
             a b H ltac:(intros x y _ _ Hxy; apply Nat.compare_eq_iff; exact Hxy)).
  reflexivity.
Qed.

Lemma key_inj : forall a b, key a = key b -> Permutation a b.
Proof.
  intros a b H. unfold key in H. simpl in H.
  injection H as H1 H2 H3 H4 H5 Hsorted.
  assert (Hs : isort Nat.compare a = isort Nat.compare b) by (apply map_of_nat_inj; exact Hsorted).
  transitivity (isort Nat.compare a); [apply isort_perm |].
  rewrite Hs. symmetry. apply isort_perm.
Qed.

(** K returns [Eq] exactly for two lists of the same chains. *)
Theorem k_eq_iff_same_branch : forall a b, k_cmp a b = Eq <-> Permutation a b.
Proof.
  intros a b. rewrite k_cmp_key. split.
  - intros H. apply key_inj. exact (lexZ_eq _ _ H).
  - intros H. rewrite (key_perm a b H). apply lexZ_refl.
Qed.

(** K is a strict total order on chain sets: antisymmetric, transitive, and
    total up to the order of the chains in a list. *)
Theorem k_strict_total :
  (forall a b, k_cmp b a = CompOpp (k_cmp a b)) /\
  (forall a b c, k_cmp a b = Lt -> k_cmp b c = Lt -> k_cmp a c = Lt) /\
  (forall a b, k_cmp a b = Lt \/ k_cmp a b = Gt \/ Permutation a b).
Proof.
  split; [| split].
  - intros a b. rewrite !k_cmp_key. apply lexZ_antisym.
  - intros a b c. rewrite !k_cmp_key. apply lexZ_trans.
  - intros a b. destruct (k_cmp a b) eqn:H.
    + right. right. apply k_eq_iff_same_branch. exact H.
    + left. reflexivity.
    + right. left. reflexivity.
Qed.

Lemma k_cmp_eq_subst : forall a b c, k_cmp a b = Eq -> k_cmp a c = k_cmp b c.
Proof.
  intros a b c H. rewrite !k_cmp_key.
  rewrite k_cmp_key in H. rewrite (lexZ_eq _ _ H). reflexivity.
Qed.

(** K puts every pinned candidate before every unpinned one. *)
Theorem k_pinned_first :
  forall a b, bpinned a = true -> bpinned b = false -> k_cmp a b = Lt.
Proof.
  intros a b Ha Hb. unfold k_cmp, numeric_cmp. rewrite Ha, Hb. reflexivity.
Qed.

(** The candidates of one merge are disjoint and not empty, so no two of
    them hold the same chains. Then the sorted list, and so every walk over
    it, depends only on the set of candidates. *)
Theorem ordered_pass_perm_invariant :
  forall (T : Type) (walk : list (list nat) -> T) cs1 cs2,
    Permutation cs1 cs2 ->
    (forall a b, In a cs1 -> In b cs1 -> Permutation a b -> a = b) ->
    walk (sort_by_k cs1) = walk (sort_by_k cs2).
Proof.
  intros T walk cs1 cs2 Hperm Hdistinct. unfold sort_by_k. f_equal.
  destruct k_strict_total as [Hanti [Htrans _]].
  apply (isort_perm_unique (list nat) k_cmp Hanti k_cmp_eq_subst Htrans cs1 cs2 Hperm).
  intros a b Ha Hb Hab. apply Hdistinct; [exact Ha | exact Hb |].
  apply k_eq_iff_same_branch. exact Hab.
Qed.

(** Canonical candidates (strictly sorted chain lists) satisfy the
    distinctness premise of [ordered_pass_perm_invariant]. *)
Lemma canonical_candidates_distinct :
  forall cs, Forall (StronglySorted lt) cs ->
    forall a b, In a cs -> In b cs -> Permutation a b -> a = b.
Proof.
  intros cs Hcs a b Ha Hb Hab.
  assert (Hsorted : forall l, StronglySorted lt l -> Sorted (le_by Nat.compare) l).
  { intros l Hl. apply StronglySorted_Sorted.
    induction Hl as [| x l Hl IH Hx].
    - constructor.
    - constructor; [exact IH |].
      eapply Forall_impl; [| exact Hx].
      intros y Hxy. unfold le_by. apply Nat.compare_lt_iff in Hxy. rewrite Hxy. discriminate. }
  rewrite Forall_forall in Hcs.
  apply (sorted_perm_unique nat Nat.compare Nat.compare_antisym
           ltac:(intros x y z Hxy; apply Nat.compare_eq_iff in Hxy; subst; reflexivity)
           ltac:(intros x y z Hxy Hyz; apply Nat.compare_lt_iff in Hxy;
                 apply Nat.compare_lt_iff in Hyz; apply Nat.compare_lt_iff; lia)).
  - apply Hsorted. exact (Hcs a Ha).
  - apply Hsorted. exact (Hcs b Hb).
  - exact Hab.
  - intros x y _ _ Hxy. apply Nat.compare_eq_iff. exact Hxy.
Qed.

End Order.

(** Negative control: a sort by the numeric key alone keeps the arrival
    order of a tie. Two unpinned candidates with no losses and one height
    then come out in the arrival order, and a walk that keeps the first of
    two conflicting candidates keeps a different one for each arrival order.
    K breaks the tie with [compare_branches], so both arrival orders give one
    list. *)
Theorem nc_position_tiebreak_not_perm_invariant :
  let pin := fun _ : nat => false in
  let zero := fun _ : nat => 0%Z in
  Permutation [[1%nat]; [2%nat]] [[2%nat]; [1%nat]] /\
  hd [] (isort (numeric_cmp pin zero zero) [[1%nat]; [2%nat]]) = [1%nat] /\
  hd [] (isort (numeric_cmp pin zero zero) [[2%nat]; [1%nat]]) = [2%nat] /\
  sort_by_k pin zero zero [[1%nat]; [2%nat]] = sort_by_k pin zero zero [[2%nat]; [1%nat]].
Proof.
  intros pin zero. split; [apply perm_swap |].
  split; [vm_compute; reflexivity |].
  split; vm_compute; reflexivity.
Qed.
