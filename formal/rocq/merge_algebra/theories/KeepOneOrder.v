(* ===========================================================================
   KeepOneOrder.v - THE P3 (block-merge determinism) proof: the 5-key
   DeployChainIndex comparator is a STRICT TOTAL ORDER whose Equal-class is
   EXACTLY the identity on the injective terminal key, so the merge's
   `min_by`/`sort` winner is NODE-IDENTICAL regardless of HashSet reseeding.

   `impl Ord for DeployChainIndex` (casper/src/rust/merging/deploy_chain_index.rs
   :151-230) is, after the fix, a 5-key lexicographic tower:

       1. Σcost           DESCENDING   (total deploy cost, economic priority)
       2. max single cost DESCENDING
       3. min deploy_id    ASCENDING   (lexicographically smallest signature)
       4. post_state_hash  ASCENDING
       5. deploys_with_cost.cmp  ASCENDING  (HashableSet<DeployIdWithCost>: Ord)

   Key 5 is the INJECTIVE Eq/Hash key itself: `PartialEq`/`Hash` are defined by
   `deploys_with_cost` equality, and `HashableSet<_>: Ord` SORTS the set (so it is
   deterministic regardless of HashSet iteration order). Because keys 1-4 are all
   FUNCTIONS of that set, `cmp a b = Eq` holds iff the two chains share the same
   set, i.e. iff they are Eq. Hence NO two DISTINCT chains ever tie.

   ---------------------------------------------------------------------------
   STRICTLY FIRMER THAN TieBreak.v
   ---------------------------------------------------------------------------
   fork_choice/theories/TieBreak.v proves the SAME determinism shape (total order
   + `output_indep_of_input_perm` + `sort_argmax_unique`) but carries a
   `NoDup (map ehash l)` PREMISE: its totality/antisymmetry only hold on lists
   whose block hashes are pairwise DISTINCT (a cryptographic-collision assumption).

   Here there is NO such premise. The injective terminal key `key` (modeled below
   as the entry itself, `entry := nat`, standing for the canonical shortlex order
   of `deploys_with_cost`) makes "distinct entries are orderable" hold BY
   CONSTRUCTION: the reflexive closure of `ord` is antisymmetric with
       cmp a b = Eq  <->  key a = key b   (`keep_one_equal_impl_eq`),
   so leb-antisymmetry (`leb_antisym`) and leb-transitivity (`leb_trans`) are
   UNCONDITIONAL, and `output_indep_of_input_perm` / `sort_argmax_unique` hold for
   ARBITRARY input lists (no NoDup, no distinctness side-condition).

   ---------------------------------------------------------------------------
   How the injective key discharges totality (no collision premise)
   ---------------------------------------------------------------------------
   The comparator is a lexicographic composition of per-key `Nat.compare`s. Each
   leaf comparator (`dcmp` for a DESCENDING key, `acmp` for ASCENDING) is a linear
   comparator: antisymmetric (`CompOpp`), Lt-transitive, and Eq-left-congruent.
   The `lexcomp` combinator PRESERVES all three (Antisym_lex / LtTrans_lex /
   EqCongR_lex). The terminal leaf is `Nat.compare a b` on the injective key
   itself, whose Eq case forces `a = b` (`Nat.compare_eq_iff`). Composing upward,
   `cmp a b = Eq` peels down to the terminal `Nat.compare a b = Eq`, hence a = b.
   With definiteness in hand, trichotomy + Lt-transitivity give a strict total
   order with NO distinctness assumption.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                        | Rust (deploy_chain_index.rs)
   ----------------------------+----------------------------------------------
   entry (= injective key)     | DeployChainIndex identified by deploys_with_cost
   g1..g4 : entry -> nat       | Σcost, max cost, min deploy_id, post_state_hash
   cmp / ord / ordb            | impl Ord::cmp (the 5-key tower)
   keep_one_equal_impl_eq      | cmp==Equal => a==b (no distinct ties => no fork)
   keep_one_total_order        | strict total order (trichotomy), UNCONDITIONAL
   sort_total_order            | irreflexive+transitive+total (a<>b form)
   output_indep_of_input_perm  | node-identical min_by/sort winner (P3 no-fork)
   sort_argmax_unique          | the unique merge winner (min_by result)

   Stdlib-only, axiom-free. `Print Assumptions keep_one_total_order` (and every
   headline) => "Closed under the global context". The entry projections g1..g4
   are Section Variables (parameters of the closed term, NOT axioms and NOT
   constraining hypotheses) -- the results hold for ALL derived-key functions, so
   the development is UNCONDITIONAL.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
Import ListNotations.

(* An entry is identified by its injective Eq/Hash key (the canonical order of
   the deploy set). We model it as a nat; keys 1-4 are FUNCTIONS of it. *)
Definition entry := nat.

(* ===========================================================================
   Section 1 - The `lexcomp` combinator over `comparison` and its algebra.
   `lexcomp c1 c2` returns c1 unless c1 is Eq, in which case it falls through to
   c2 -- exactly the `if k_cmp != Equal { return k_cmp }` tower in `impl Ord`.
   =========================================================================== *)

Definition lexcomp (c1 c2 : comparison) : comparison :=
  match c1 with Eq => c2 | _ => c1 end.

Lemma lexcomp_Lt_iff :
  forall x y, lexcomp x y = Lt <-> x = Lt \/ (x = Eq /\ y = Lt).
Proof.
  intros x y; destruct x; simpl; split; intro H.
  - right; split; [reflexivity | exact H].
  - destruct H as [H | [_ H]]; [discriminate | exact H].
  - left; reflexivity.
  - reflexivity.
  - discriminate.
  - destruct H as [H | [H _]]; discriminate.
Qed.

Lemma lexcomp_Eq_iff :
  forall x y, lexcomp x y = Eq <-> x = Eq /\ y = Eq.
Proof.
  intros x y; destruct x; simpl; split; intro H.
  - split; [reflexivity | exact H].
  - destruct H as [_ H]; exact H.
  - discriminate.
  - destruct H as [H _]; discriminate.
  - discriminate.
  - destruct H as [H _]; discriminate.
Qed.

Lemma lexcomp_antisym_val :
  forall u1 u2 v1 v2 : comparison,
    v1 = CompOpp u1 -> v2 = CompOpp u2 ->
    lexcomp v1 v2 = CompOpp (lexcomp u1 u2).
Proof. intros u1 u2 v1 v2 H1 H2; subst; destruct u1; simpl; reflexivity. Qed.

(* A linear-comparator interface: antisymmetry, Lt-transitivity, Eq-left-
   congruence. Every leaf below satisfies it; `lexcomp` preserves it. *)
Definition Antisym (f : entry -> entry -> comparison) : Prop :=
  forall a b, f b a = CompOpp (f a b).
Definition LtTrans (f : entry -> entry -> comparison) : Prop :=
  forall a b c, f a b = Lt -> f b c = Lt -> f a c = Lt.
Definition EqCongR (f : entry -> entry -> comparison) : Prop :=
  forall a b c, f a b = Eq -> f a c = f b c.

Lemma Antisym_lex :
  forall f1 f2, Antisym f1 -> Antisym f2 ->
    Antisym (fun a b => lexcomp (f1 a b) (f2 a b)).
Proof. intros f1 f2 A1 A2 a b. apply lexcomp_antisym_val; [apply A1 | apply A2]. Qed.

Lemma EqCongR_lex :
  forall f1 f2, EqCongR f1 -> EqCongR f2 ->
    EqCongR (fun a b => lexcomp (f1 a b) (f2 a b)).
Proof.
  intros f1 f2 E1 E2 a b c H.
  apply lexcomp_Eq_iff in H. destruct H as [H1 H2].
  rewrite (E1 a b c H1). rewrite (E2 a b c H2). reflexivity.
Qed.

Lemma LtTrans_lex :
  forall f1 f2, Antisym f1 -> LtTrans f1 -> EqCongR f1 -> LtTrans f2 ->
    LtTrans (fun a b => lexcomp (f1 a b) (f2 a b)).
Proof.
  intros f1 f2 A1 T1 E1 T2 a b c Hab Hbc.
  apply lexcomp_Lt_iff in Hab. apply lexcomp_Lt_iff in Hbc. apply lexcomp_Lt_iff.
  destruct Hab as [Hab | [Hab1 Hab2]]; destruct Hbc as [Hbc | [Hbc1 Hbc2]].
  - (* f1 a b = Lt, f1 b c = Lt *) left. apply (T1 a b c Hab Hbc).
  - (* f1 a b = Lt, f1 b c = Eq *)
    left.
    pose proof (E1 b c a Hbc1) as Eb.     (* f1 b a = f1 c a *)
    pose proof (A1 a b) as Aab.           (* f1 b a = CompOpp (f1 a b) *)
    rewrite Hab in Aab. simpl in Aab.     (* f1 b a = Gt *)
    rewrite Aab in Eb.                    (* Gt = f1 c a *)
    pose proof (A1 c a) as Aca.           (* f1 a c = CompOpp (f1 c a) *)
    rewrite <- Eb in Aca. simpl in Aca.   (* f1 a c = Lt *)
    exact Aca.
  - (* f1 a b = Eq, f1 b c = Lt *)
    left.
    pose proof (E1 a b c Hab1) as Ea.     (* f1 a c = f1 b c *)
    rewrite Hbc in Ea. exact Ea.
  - (* f1 a b = Eq, f1 b c = Eq: fall through to the tail comparator *)
    right.
    pose proof (E1 a b c Hab1) as Ea.     (* f1 a c = f1 b c *)
    rewrite Hbc1 in Ea.                   (* f1 a c = Eq *)
    split; [exact Ea | apply (T2 a b c Hab2 Hbc2)].
Qed.

(* ===========================================================================
   Section 2 - Leaf comparators: DESCENDING (dcmp) and ASCENDING (acmp) on a key.
   =========================================================================== *)

Definition dcmp (k : entry -> nat) : entry -> entry -> comparison :=
  fun a b => Nat.compare (k b) (k a).   (* DESC: a before b iff k a > k b *)
Definition acmp (k : entry -> nat) : entry -> entry -> comparison :=
  fun a b => Nat.compare (k a) (k b).   (* ASC:  a before b iff k a < k b *)

Lemma dcmp_antisym : forall k, Antisym (dcmp k).
Proof. intros k a b. unfold dcmp. apply Nat.compare_antisym. Qed.
Lemma acmp_antisym : forall k, Antisym (acmp k).
Proof. intros k a b. unfold acmp. apply Nat.compare_antisym. Qed.

Lemma dcmp_lttrans : forall k, LtTrans (dcmp k).
Proof.
  intros k a b c H1 H2. unfold dcmp in *.
  apply Nat.compare_lt_iff in H1. apply Nat.compare_lt_iff in H2.
  apply Nat.compare_lt_iff. lia.
Qed.
Lemma acmp_lttrans : forall k, LtTrans (acmp k).
Proof.
  intros k a b c H1 H2. unfold acmp in *.
  apply Nat.compare_lt_iff in H1. apply Nat.compare_lt_iff in H2.
  apply Nat.compare_lt_iff. lia.
Qed.

Lemma dcmp_eqcongr : forall k, EqCongR (dcmp k).
Proof.
  intros k a b c H. unfold dcmp in *.
  apply Nat.compare_eq_iff in H. rewrite H. reflexivity.
Qed.
Lemma acmp_eqcongr : forall k, EqCongR (acmp k).
Proof.
  intros k a b c H. unfold acmp in *.
  apply Nat.compare_eq_iff in H. rewrite H. reflexivity.
Qed.

(* ===========================================================================
   Section 3 - The 5-key comparator, assembled bottom-up, and its linearity.
   =========================================================================== *)

Section KeepOne.
  (* keys 1-4 as arbitrary functions of the entry (= injective key). NO
     hypothesis constrains them: the results hold for ALL g1..g4, so the whole
     development is UNCONDITIONAL. *)
  Variables g1 g2 g3 g4 : entry -> nat.

  Definition tcmp5 : entry -> entry -> comparison := fun a b => Nat.compare a b.
  Definition tcmp4 : entry -> entry -> comparison :=
    fun a b => lexcomp (acmp g4 a b) (tcmp5 a b).
  Definition tcmp3 : entry -> entry -> comparison :=
    fun a b => lexcomp (acmp g3 a b) (tcmp4 a b).
  Definition tcmp2 : entry -> entry -> comparison :=
    fun a b => lexcomp (dcmp g2 a b) (tcmp3 a b).
  Definition cmp : entry -> entry -> comparison :=
    fun a b => lexcomp (dcmp g1 a b) (tcmp2 a b).

  (* --- terminal leaf (injective key) --- *)
  Lemma tcmp5_antisym : Antisym tcmp5.
  Proof. intros a b. unfold tcmp5. apply Nat.compare_antisym. Qed.
  Lemma tcmp5_lttrans : LtTrans tcmp5.
  Proof.
    intros a b c H1 H2. unfold tcmp5 in *.
    apply Nat.compare_lt_iff in H1. apply Nat.compare_lt_iff in H2.
    apply Nat.compare_lt_iff. lia.
  Qed.
  Lemma tcmp5_eqcongr : EqCongR tcmp5.
  Proof.
    intros a b c H. unfold tcmp5 in *.
    apply Nat.compare_eq_iff in H. rewrite H. reflexivity.
  Qed.

  (* --- bubble the linear-comparator interface up the tower --- *)
  Lemma tcmp4_antisym : Antisym tcmp4.
  Proof. unfold tcmp4. apply (Antisym_lex (acmp g4) tcmp5); [apply acmp_antisym | apply tcmp5_antisym]. Qed.
  Lemma tcmp4_eqcongr : EqCongR tcmp4.
  Proof. unfold tcmp4. apply (EqCongR_lex (acmp g4) tcmp5); [apply acmp_eqcongr | apply tcmp5_eqcongr]. Qed.
  Lemma tcmp4_lttrans : LtTrans tcmp4.
  Proof.
    unfold tcmp4. apply (LtTrans_lex (acmp g4) tcmp5);
      [apply acmp_antisym | apply acmp_lttrans | apply acmp_eqcongr | apply tcmp5_lttrans].
  Qed.

  Lemma tcmp3_antisym : Antisym tcmp3.
  Proof. unfold tcmp3. apply (Antisym_lex (acmp g3) tcmp4); [apply acmp_antisym | apply tcmp4_antisym]. Qed.
  Lemma tcmp3_eqcongr : EqCongR tcmp3.
  Proof. unfold tcmp3. apply (EqCongR_lex (acmp g3) tcmp4); [apply acmp_eqcongr | apply tcmp4_eqcongr]. Qed.
  Lemma tcmp3_lttrans : LtTrans tcmp3.
  Proof.
    unfold tcmp3. apply (LtTrans_lex (acmp g3) tcmp4);
      [apply acmp_antisym | apply acmp_lttrans | apply acmp_eqcongr | apply tcmp4_lttrans].
  Qed.

  Lemma tcmp2_antisym : Antisym tcmp2.
  Proof. unfold tcmp2. apply (Antisym_lex (dcmp g2) tcmp3); [apply dcmp_antisym | apply tcmp3_antisym]. Qed.
  Lemma tcmp2_eqcongr : EqCongR tcmp2.
  Proof. unfold tcmp2. apply (EqCongR_lex (dcmp g2) tcmp3); [apply dcmp_eqcongr | apply tcmp3_eqcongr]. Qed.
  Lemma tcmp2_lttrans : LtTrans tcmp2.
  Proof.
    unfold tcmp2. apply (LtTrans_lex (dcmp g2) tcmp3);
      [apply dcmp_antisym | apply dcmp_lttrans | apply dcmp_eqcongr | apply tcmp3_lttrans].
  Qed.

  Lemma cmp_antisym : Antisym cmp.
  Proof. unfold cmp. apply (Antisym_lex (dcmp g1) tcmp2); [apply dcmp_antisym | apply tcmp2_antisym]. Qed.
  Lemma cmp_eqcongr : EqCongR cmp.
  Proof. unfold cmp. apply (EqCongR_lex (dcmp g1) tcmp2); [apply dcmp_eqcongr | apply tcmp2_eqcongr]. Qed.
  Lemma cmp_lttrans : LtTrans cmp.
  Proof.
    unfold cmp. apply (LtTrans_lex (dcmp g1) tcmp2);
      [apply dcmp_antisym | apply dcmp_lttrans | apply dcmp_eqcongr | apply tcmp2_lttrans].
  Qed.

  (* DEFINITENESS: cmp a b = Eq forces a = b -- because the tower bottoms out at
     the INJECTIVE terminal key `Nat.compare a b`. This is the property TieBreak.v
     can only obtain under a NoDup hash-distinctness premise. *)
  Lemma cmp_eq_impl : forall a b, cmp a b = Eq -> a = b.
  Proof.
    intros a b H. unfold cmp in H. apply lexcomp_Eq_iff in H. destruct H as [_ H].
    unfold tcmp2 in H. apply lexcomp_Eq_iff in H. destruct H as [_ H].
    unfold tcmp3 in H. apply lexcomp_Eq_iff in H. destruct H as [_ H].
    unfold tcmp4 in H. apply lexcomp_Eq_iff in H. destruct H as [_ H].
    unfold tcmp5 in H. apply Nat.compare_eq_iff in H. exact H.
  Qed.

  Lemma cmp_refl : forall a, cmp a a = Eq.
  Proof.
    intro a. pose proof (cmp_antisym a a) as H.
    remember (cmp a a) as x eqn:E. destruct x; simpl in H; [reflexivity | discriminate H | discriminate H].
  Qed.

  (* ===========================================================================
     Section 4 - The strict order `ord` and its total-order laws (UNCONDITIONAL).
     =========================================================================== *)

  Definition ord (a b : entry) : Prop := cmp a b = Lt.

  Lemma ord_irrefl : forall a, ~ ord a a.
  Proof. intros a H. unfold ord in H. rewrite cmp_refl in H. discriminate. Qed.

  Lemma ord_trans : forall a b c, ord a b -> ord b c -> ord a c.
  Proof. intros a b c H1 H2. exact (cmp_lttrans a b c H1 H2). Qed.

  Lemma ord_asym : forall a b, ord a b -> ~ ord b a.
  Proof.
    intros a b H1 H2. unfold ord in *.
    pose proof (cmp_antisym a b) as A. rewrite H1 in A. simpl in A.
    rewrite A in H2. discriminate.
  Qed.

  (* Trichotomy -- the total-order content, WITHOUT any distinctness premise. *)
  Lemma ord_total : forall a b, ord a b \/ a = b \/ ord b a.
  Proof.
    intros a b. unfold ord. destruct (cmp a b) eqn:E.
    - right; left. apply cmp_eq_impl; exact E.
    - left; reflexivity.
    - right; right. pose proof (cmp_antisym a b) as A. rewrite E in A. simpl in A. exact A.
  Qed.

  Lemma ord_total_ne : forall a b, a <> b -> ord a b \/ ord b a.
  Proof.
    intros a b Hne. destruct (ord_total a b) as [H | [H | H]];
      [left; exact H | contradiction | right; exact H].
  Qed.

  (* THE injectivity of the Equal-class (subset direction): cmp Equal => Eq. *)
  Theorem keep_one_equal_impl_eq : forall a b, cmp a b = Eq -> a = b.
  Proof. exact cmp_eq_impl. Qed.

  (* THE headline strict total order (trichotomy form), UNCONDITIONAL. *)
  Theorem keep_one_total_order :
    (forall a, ~ ord a a)
    /\ (forall a b c, ord a b -> ord b c -> ord a c)
    /\ (forall a b, ord a b \/ a = b \/ ord b a).
  Proof. split; [exact ord_irrefl | split; [exact ord_trans | exact ord_total]]. Qed.

  (* TieBreak.v-parity statement (irreflexive + transitive + total on a<>b),
     here UNCONDITIONAL (TieBreak needs `ehash a <> ehash b`, i.e. NoDup). *)
  Theorem sort_total_order :
    (forall a, ~ ord a a)
    /\ (forall a b c, ord a b -> ord b c -> ord a c)
    /\ (forall a b, a <> b -> ord a b \/ ord b a).
  Proof. split; [exact ord_irrefl | split; [exact ord_trans | exact ord_total_ne]]. Qed.

  (* ===========================================================================
     Section 5 - the "<=" companion; antisymmetry and transitivity WITHOUT NoDup.
     =========================================================================== *)

  Definition ordb (a b : entry) : bool :=
    match cmp a b with Lt => true | _ => false end.

  Lemma ordb_true_iff : forall a b, ordb a b = true <-> ord a b.
  Proof.
    intros a b. unfold ordb, ord. destruct (cmp a b) eqn:E; split; intro H;
      solve [reflexivity | discriminate].
  Qed.

  Definition leb (a b : entry) : bool := negb (ordb b a).

  Lemma leb_true_iff : forall a b, leb a b = true <-> ~ ord b a.
  Proof.
    intros a b. unfold leb. rewrite negb_true_iff. split; intro H.
    - intro Ho. apply ordb_true_iff in Ho. rewrite Ho in H. discriminate.
    - destruct (ordb b a) eqn:E; [exfalso; apply H; apply ordb_true_iff; exact E | reflexivity].
  Qed.

  Lemma leb_total : forall a b, leb a b = true \/ leb b a = true.
  Proof.
    intros a b. destruct (ord_total a b) as [H | [H | H]].
    - left. apply leb_true_iff. apply ord_asym; exact H.
    - left. apply leb_true_iff. subst b. apply ord_irrefl.
    - right. apply leb_true_iff. apply ord_asym; exact H.
  Qed.

  (* Antisymmetry of <= is UNCONDITIONAL (TieBreak's `leb_antisym` needs distinct
     hashes to conclude a = b; ours uses definiteness `cmp_eq_impl`). *)
  Lemma leb_antisym : forall a b, leb a b = true -> leb b a = true -> a = b.
  Proof.
    intros a b Hab Hba. apply leb_true_iff in Hab. apply leb_true_iff in Hba.
    destruct (ord_total a b) as [H | [H | H]]; [contradiction | exact H | contradiction].
  Qed.

  (* Transitivity of <= is UNCONDITIONAL (TieBreak's `leb_ord_trans_distinct`
     REQUIRES pairwise-distinct hashes; ours needs none). *)
  Lemma leb_trans : forall a b c, leb a b = true -> leb b c = true -> leb a c = true.
  Proof.
    intros a b c Hab Hbc. apply leb_true_iff. intro Hca.
    apply leb_true_iff in Hab. apply leb_true_iff in Hbc.
    assert (Hab' : ord a b \/ a = b).
    { destruct (ord_total a b) as [H | [H | H]]; [left; exact H | right; exact H | contradiction]. }
    assert (Hbc' : ord b c \/ b = c).
    { destruct (ord_total b c) as [H | [H | H]]; [left; exact H | right; exact H | contradiction]. }
    destruct Hab' as [Hab' | Hab']; destruct Hbc' as [Hbc' | Hbc'].
    - exact (ord_asym a c (ord_trans a b c Hab' Hbc') Hca).
    - subst c. exact (ord_asym a b Hab' Hca).
    - subst b. exact (ord_asym a c Hbc' Hca).
    - subst b; subst c. exact (ord_irrefl a Hca).
  Qed.

  (* ===========================================================================
     Section 6 - insertion sort and its determinism / argmax uniqueness.
     =========================================================================== *)

  Fixpoint insert (x : entry) (l : list entry) : list entry :=
    match l with
    | [] => [x]
    | y :: rest => if ordb x y then x :: y :: rest else y :: insert x rest
    end.

  Fixpoint sort (l : list entry) : list entry :=
    match l with
    | [] => []
    | x :: rest => insert x (sort rest)
    end.

  Lemma insert_perm : forall x l, Permutation (insert x l) (x :: l).
  Proof.
    intros x l. induction l as [| y rest IH]; simpl.
    - apply Permutation_refl.
    - destruct (ordb x y).
      + apply Permutation_refl.
      + eapply Permutation_trans; [apply perm_skip; exact IH | apply perm_swap].
  Qed.

  Theorem sort_is_permutation : forall l, Permutation (sort l) l.
  Proof.
    induction l as [| x rest IH]; simpl.
    - apply Permutation_refl.
    - eapply Permutation_trans; [apply insert_perm | apply perm_skip; exact IH].
  Qed.

  Fixpoint is_sorted (l : list entry) : bool :=
    match l with
    | [] => true
    | x :: rest =>
        match rest with
        | [] => true
        | y :: _ => leb x y && is_sorted rest
        end
    end.

  Lemma is_sorted_tail : forall a t, is_sorted (a :: t) = true -> is_sorted t = true.
  Proof.
    intros a t H. destruct t as [| c t']; simpl in *.
    - reflexivity.
    - apply andb_true_iff in H. destruct H as [_ H]. exact H.
  Qed.

  Lemma insert_hd :
    forall x a l, leb a x = true -> is_sorted (a :: l) = true ->
      is_sorted (a :: insert x l) = true.
  Proof.
    intros x a l. revert a. induction l as [| y rest IH]; intros a Hax Hsort.
    - simpl. rewrite Hax. reflexivity.
    - simpl insert. destruct (ordb x y) eqn:E.
      + simpl. rewrite Hax. simpl.
        assert (Hxy : leb x y = true).
        { unfold leb. apply negb_true_iff. unfold ordb.
          apply ordb_true_iff in E. pose proof (ord_asym x y E) as Hna.
          destruct (cmp y x) eqn:Eyx; try reflexivity.
          exfalso. apply Hna. unfold ord. exact Eyx. }
        rewrite Hxy. simpl. apply is_sorted_tail in Hsort. exact Hsort.
      + assert (Hay : leb a y = true).
        { simpl in Hsort. apply andb_true_iff in Hsort. destruct Hsort as [H _]. exact H. }
        assert (Hyx : leb y x = true).
        { unfold leb. apply negb_true_iff. exact E. }
        assert (Hsyrest : is_sorted (y :: rest) = true) by (apply is_sorted_tail in Hsort; exact Hsort).
        specialize (IH y Hyx Hsyrest).
        simpl. rewrite Hay. simpl. simpl in IH. exact IH.
  Qed.

  Lemma insert_sorted :
    forall x l, is_sorted l = true -> is_sorted (insert x l) = true.
  Proof.
    intros x l Hsort. destruct l as [| y rest].
    - simpl. reflexivity.
    - simpl insert. destruct (ordb x y) eqn:E.
      + simpl.
        assert (Hxy : leb x y = true).
        { unfold leb. apply negb_true_iff. unfold ordb.
          apply ordb_true_iff in E. pose proof (ord_asym x y E) as Hna.
          destruct (cmp y x) eqn:Eyx; try reflexivity.
          exfalso. apply Hna. unfold ord. exact Eyx. }
        rewrite Hxy. simpl. exact Hsort.
      + assert (Hyx : leb y x = true).
        { unfold leb. apply negb_true_iff. exact E. }
        apply (insert_hd x y rest Hyx Hsort).
  Qed.

  Theorem sort_sorted : forall l, is_sorted (sort l) = true.
  Proof.
    induction l as [| x rest IH]; simpl.
    - reflexivity.
    - apply insert_sorted. exact IH.
  Qed.

  (* In a sorted list, the head is <= every later element. NO NoDup premise. *)
  Lemma sorted_head_min :
    forall t a, is_sorted (a :: t) = true -> Forall (fun b => leb a b = true) t.
  Proof.
    induction t as [| c t' IH]; intros a Hsort.
    - apply Forall_nil.
    - assert (Hac : leb a c = true).
      { simpl in Hsort. apply andb_true_iff in Hsort. destruct Hsort as [H _]. exact H. }
      assert (Hsct : is_sorted (c :: t') = true) by (apply is_sorted_tail in Hsort; exact Hsort).
      specialize (IH c Hsct).
      apply Forall_cons; [exact Hac |].
      rewrite Forall_forall in IH. rewrite Forall_forall. intros b Hbin.
      apply (leb_trans a c b Hac). apply IH; exact Hbin.
  Qed.

  (* THE determinism kernel: two sorted permutations are EQUAL. NO NoDup premise
     (TieBreak's `sorted_perm_nodup_eq` requires `NoDup (map ehash x)`). *)
  Lemma sorted_perm_eq :
    forall x y, is_sorted x = true -> is_sorted y = true -> Permutation x y -> x = y.
  Proof.
    induction x as [| a t IH]; intros y Hsx Hsy Hperm.
    - apply Permutation_nil in Hperm. symmetry; exact Hperm.
    - destruct y as [| b s].
      { exfalso. apply (Permutation_nil_cons (Permutation_sym Hperm)). }
      assert (Hab : a = b).
      { assert (Hain : In a (b :: s)) by (apply (Permutation_in a Hperm); left; reflexivity).
        assert (Hbin : In b (a :: t)) by (apply (Permutation_in b (Permutation_sym Hperm)); left; reflexivity).
        assert (Haleb : leb a b = true).
        { destruct Hbin as [Hbin | Hbin].
          - subst b. destruct (leb_total a a) as [H | H]; exact H.
          - pose proof (sorted_head_min t a Hsx) as Hfa.
            rewrite Forall_forall in Hfa. apply Hfa; exact Hbin. }
        assert (Hblea : leb b a = true).
        { destruct Hain as [Hain | Hain].
          - subst b. destruct (leb_total a a) as [H | H]; exact H.
          - pose proof (sorted_head_min s b Hsy) as Hfb.
            rewrite Forall_forall in Hfb. apply Hfb; exact Hain. }
        apply (leb_antisym a b Haleb Hblea). }
      subst b. f_equal.
      apply IH.
      + apply is_sorted_tail in Hsx; exact Hsx.
      + apply is_sorted_tail in Hsy; exact Hsy.
      + apply (Permutation_cons_inv Hperm).
  Qed.

  (* THE P3 DETERMINISM LEMMA: permuted merge inputs sort to the IDENTICAL list.
     Every node's `min_by`/`sort` winner is byte-identical regardless of HashSet
     reseeding -> no fork. NO NoDup premise (the injective key does the work). *)
  Theorem output_indep_of_input_perm :
    forall l l', Permutation l l' -> sort l = sort l'.
  Proof.
    intros l l' Hperm. apply sorted_perm_eq.
    - apply sort_sorted.
    - apply sort_sorted.
    - eapply Permutation_trans; [apply sort_is_permutation |].
      eapply Permutation_trans; [exact Hperm | apply Permutation_sym; apply sort_is_permutation].
  Qed.

  (* The head is the UNIQUE maximum (the merge winner). NO NoDup premise. *)
  Theorem sort_argmax_unique :
    forall l,
      match sort l with
      | [] => l = []
      | w :: _ => In w l /\ forall e, In e l -> e = w \/ ord w e
      end.
  Proof.
    intros l. destruct (sort l) as [| w s] eqn:Es.
    - apply Permutation_nil. rewrite <- Es. apply sort_is_permutation.
    - assert (Hperm : Permutation (sort l) l) by apply sort_is_permutation.
      rewrite Es in Hperm.
      assert (Hsorted : is_sorted (w :: s) = true) by (rewrite <- Es; apply sort_sorted).
      split.
      + apply (Permutation_in w Hperm); left; reflexivity.
      + intros e Hin.
        assert (HinWS : In e (w :: s)) by (apply (Permutation_in e (Permutation_sym Hperm)); exact Hin).
        destruct HinWS as [He | He].
        * left; symmetry; exact He.
        * pose proof (sorted_head_min s w Hsorted) as Hfa. rewrite Forall_forall in Hfa.
          assert (Hwe : leb w e = true) by (apply Hfa; exact He).
          apply leb_true_iff in Hwe.
          destruct (ord_total w e) as [H | [H | H]].
          -- right; exact H.
          -- left; symmetry; exact H.
          -- exfalso; apply Hwe; exact H.
  Qed.

  (* The REJECTED-LOSERS list (everything the greedy keep-one does NOT keep --
     the tail after the unique winner is removed) is likewise a pure function of
     the input SET: permuted merge inputs drop the identical winner and leave the
     byte-identical remainder. This is the rejected-deploy-set half of P3/R-KEEP1
     (the winner half is sort_argmax_unique): a direct corollary of
     output_indep_of_input_perm via `tl`. *)
  Corollary keep_one_reject_losers_deterministic :
    forall l l', Permutation l l' -> tl (sort l) = tl (sort l').
  Proof.
    intros l l' H. rewrite (output_indep_of_input_perm l l' H). reflexivity.
  Qed.

End KeepOne.
