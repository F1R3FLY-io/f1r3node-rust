(* ===========================================================================
   TieBreak.v - THE S1 (fork-prevention) proof: the ranked fork-choice output
   is NODE-IDENTICAL because it is a sort under a TOTAL order on distinct keys.

   `rank_forkchoices` ranks candidate tips with
   `ListOps::sort_by_with_decreasing_order` (shared/src/rust/shared/list_ops.rs
   :44-67): a lexicographic order on (score, hash) that is

       score DESCENDING, then hash ASCENDING

   (Scala `Ordering.Tuple2(Ordering[Long].reverse, hashBytes)`). A block hash is
   a cryptographic digest, so distinct candidates have DISTINCT hashes; on
   distinct keys this lex order is a STRICT TOTAL order, and a sorted permutation
   under a strict total order is UNIQUE. Hence every honest node - regardless of
   the (HashSet-nondeterministic) order in which it enumerates candidates -
   produces the IDENTICAL ranked list, so the SAME ghost main parent (tips[0]).
   That uniqueness is the safety S1 content (no fork from enumeration order).

   Proved here (Stdlib-only, axiom-free):
     * `sort_total_order`         - the order is irreflexive + transitive + total
                                    on distinct hashes (a strict total order);
     * `sort_is_permutation`      - the sort permutes its input;
     * `sort_sorted`              - the sort's output is sorted;
     * `output_indep_of_input_perm` - THE determinism lemma: on distinct hashes,
                                    permuted inputs sort to the IDENTICAL list;
     * `sort_argmax_unique`       - the head is the unique maximum (the winner).

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                       | Rust
   ---------------------------+------------------------------------------------
   entry = (score, hash)      | (i64 score, ByteString hash) tuple
   ord / ordb                 | Ordering[Long].reverse then hash asc (list_ops:55)
   sort (insertion)           | ListOps::sort_by_with_decreasing_order (:44)
   output_indep_of_input_perm | node-identical ranked tips (S1 no-fork)
   sort_argmax_unique         | tips[0] = the GHOST head (snapshot.rs:317-323)
   =========================================================================== *)

(* CAVEAT - tips[0] is the GHOST head, NOT necessarily the block's MAIN PARENT.
   `snapshot.rs:317-323` takes `tips.into_iter().next()` as `ghost_main_parent` and
   :325-331 sorts the parents so it comes first, but :332 then runs
   `prefer_deploy_support_main_parent` (:124-185), which can PROMOTE a
   deploy-carrying branch to index 0 and override it. The estimator/ranking results
   in THIS module are unaffected (they are about `tips`, which is what they say);
   the consumer-side re-ordering is modeled in GuardBridge.v seam (3) - see
   `main_parent_pipeline_deterministic` and the computable refutation
   `pipeline_head_may_differ_from_ghost`. *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
Import ListNotations.

(* An entry ranked by the estimator: (cumulative score, block hash). *)
Definition entry := (nat * nat)%type.
Definition escore (e : entry) : nat := fst e.
Definition ehash  (e : entry) : nat := snd e.

(* ===========================================================================
   Section 1 - The strict order: score DESCENDING, then hash ASCENDING.
   `ord a b` reads "a outranks b" (a sorts strictly before b).
   =========================================================================== *)

Definition ord (a b : entry) : Prop :=
  escore b < escore a \/ (escore b = escore a /\ ehash a < ehash b).

Definition ordb (a b : entry) : bool :=
  (escore b <? escore a) || ((escore b =? escore a) && (ehash a <? ehash b)).

Lemma ordb_true_iff : forall a b, ordb a b = true <-> ord a b.
Proof.
  intros a b. unfold ordb, ord. rewrite orb_true_iff, andb_true_iff.
  rewrite Nat.ltb_lt, Nat.eqb_eq, Nat.ltb_lt. reflexivity.
Qed.

Lemma ordb_false_iff : forall a b, ordb a b = false <-> ~ ord a b.
Proof.
  intros a b. split; intro H.
  - intro Hord. apply (ordb_true_iff a b) in Hord. rewrite Hord in H. discriminate.
  - destruct (ordb a b) eqn:E; [| reflexivity].
    exfalso. apply H. apply (ordb_true_iff a b). exact E.
Qed.

Lemma ord_irrefl : forall a, ~ ord a a.
Proof. intros a [H | [_ H]]; lia. Qed.

Lemma ord_trans : forall a b c, ord a b -> ord b c -> ord a c.
Proof.
  intros a b c [Hab | [Hab1 Hab2]] [Hbc | [Hbc1 Hbc2]]; unfold ord; lia.
Qed.

(* Totality on DISTINCT hashes: any two entries with different hashes are
   ordered one way or the other (a cryptographic digest makes hashes distinct). *)
Lemma ord_total : forall a b, ehash a <> ehash b -> ord a b \/ ord b a.
Proof.
  intros a b Hne. unfold ord.
  destruct (Nat.lt_trichotomy (escore a) (escore b)) as [Hs | [Hs | Hs]].
  - right. left. exact Hs.
  - destruct (Nat.lt_trichotomy (ehash a) (ehash b)) as [Hh | [Hh | Hh]].
    + left. right. split; [lia | exact Hh].
    + contradiction.
    + right. right. split; [lia | exact Hh].
  - left. left. exact Hs.
Qed.

Lemma ord_asym : forall a b, ord a b -> ~ ord b a.
Proof.
  intros a b Hab Hba. apply (ord_irrefl a). eapply ord_trans; eauto.
Qed.

(* S1 core: the tie-break order is a STRICT TOTAL order on distinct hashes. *)
Theorem sort_total_order :
  (forall a, ~ ord a a)
  /\ (forall a b c, ord a b -> ord b c -> ord a c)
  /\ (forall a b, ehash a <> ehash b -> ord a b \/ ord b a).
Proof.
  split; [exact ord_irrefl | split; [exact ord_trans | exact ord_total]].
Qed.

(* ===========================================================================
   Section 2 - The "<=" companion and its antisymmetry / distinct-key transitivity
   `leb_ord a b = true` reads "a does not sort after b" (a <= b).
   =========================================================================== *)

Definition leb_ord (a b : entry) : bool := negb (ordb b a).

Lemma leb_ord_true_iff : forall a b, leb_ord a b = true <-> ~ ord b a.
Proof.
  intros a b. unfold leb_ord. rewrite negb_true_iff. apply ordb_false_iff.
Qed.

(* Antisymmetry: a <= b and b <= a force a = b (as (score,hash) pairs). *)
Lemma leb_antisym : forall a b, leb_ord a b = true -> leb_ord b a = true -> a = b.
Proof.
  intros a b Hab Hba.
  apply (leb_ord_true_iff a b) in Hab.   (* ~ ord b a *)
  apply (leb_ord_true_iff b a) in Hba.   (* ~ ord a b *)
  assert (Hh : ehash a = ehash b).
  { destruct (Nat.eq_dec (ehash a) (ehash b)) as [He | Hne]; [exact He |].
    exfalso. destruct (ord_total a b Hne) as [H | H]; [apply Hba | apply Hab]; exact H. }
  (* From ~ord a b and ~ord b a extract score equality. *)
  unfold ord in Hab, Hba.
  assert (Hs : escore a = escore b) by lia.
  destruct a as [sa ha]; destruct b as [sb hb].
  unfold escore, ehash in *; simpl in *. subst. reflexivity.
Qed.

(* On distinct keys, "<=" is transitive (needs totality, hence distinctness). *)
Lemma leb_ord_trans_distinct :
  forall a b c,
    ehash a <> ehash b -> ehash b <> ehash c -> ehash a <> ehash c ->
    leb_ord a b = true -> leb_ord b c = true -> leb_ord a c = true.
Proof.
  intros a b c Hab_h Hbc_h Hac_h Hab Hbc.
  apply (leb_ord_true_iff a b) in Hab.   (* ~ ord b a *)
  apply (leb_ord_true_iff b c) in Hbc.   (* ~ ord c b *)
  apply (leb_ord_true_iff a c).          (* goal: ~ ord c a *)
  intro Hca.                             (* assume ord c a *)
  (* a <= b, distinct -> ord a b ; b <= c, distinct -> ord b c *)
  assert (Hab' : ord a b).
  { destruct (ord_total a b Hab_h) as [H | H]; [exact H | contradiction]. }
  assert (Hbc' : ord b c).
  { destruct (ord_total b c Hbc_h) as [H | H]; [exact H | contradiction]. }
  (* ord c a, ord a b, ord b c -> ord c c, contradiction *)
  apply (ord_irrefl c).
  eapply ord_trans; [exact Hca |]. eapply ord_trans; [exact Hab' | exact Hbc'].
Qed.

(* ===========================================================================
   Section 3 - Insertion sort (a hand-rolled sort, per the spec's leeway)
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

(* ---- Permutation ---------------------------------------------------------- *)

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

(* ---- Sortedness ----------------------------------------------------------- *)

Fixpoint is_sorted (l : list entry) : bool :=
  match l with
  | [] => true
  | x :: rest =>
      match rest with
      | [] => true
      | y :: _ => leb_ord x y && is_sorted rest
      end
  end.

Lemma is_sorted_tail : forall a t, is_sorted (a :: t) = true -> is_sorted t = true.
Proof.
  intros a t H. destruct t as [| c t']; simpl in *.
  - reflexivity.
  - apply andb_true_iff in H. destruct H as [_ H]. exact H.
Qed.

(* If a <= x and (a :: l) is sorted, then (a :: insert x l) is sorted. *)
Lemma insert_hd :
  forall x a l, leb_ord a x = true -> is_sorted (a :: l) = true ->
    is_sorted (a :: insert x l) = true.
Proof.
  intros x a l. revert a. induction l as [| y rest IH]; intros a Hax Hsort.
  - simpl. rewrite Hax. reflexivity.
  - simpl insert. destruct (ordb x y) eqn:E.
    + (* x before y : a :: x :: y :: rest *)
      simpl. rewrite Hax. simpl.
      assert (Hxy : leb_ord x y = true).
      { unfold leb_ord. apply negb_true_iff. apply ordb_false_iff.
        apply ord_asym. apply (ordb_true_iff x y). exact E. }
      rewrite Hxy. simpl.
      (* remaining: is_sorted (y :: rest), from Hsort = is_sorted (a::y::rest) *)
      apply is_sorted_tail in Hsort. exact Hsort.
    + (* y <= x : a :: y :: insert x rest *)
      assert (Hay : leb_ord a y = true).
      { simpl in Hsort. apply andb_true_iff in Hsort. destruct Hsort as [H _]. exact H. }
      assert (Hyx : leb_ord y x = true).
      { unfold leb_ord. apply negb_true_iff. exact E. }
      assert (Hsyrest : is_sorted (y :: rest) = true) by (apply is_sorted_tail in Hsort; exact Hsort).
      specialize (IH y Hyx Hsyrest).            (* is_sorted (y :: insert x rest) *)
      simpl. rewrite Hay. simpl.
      (* goal: is_sorted (y :: insert x rest) = true, which is IH *)
      simpl in IH. exact IH.
Qed.

Lemma insert_sorted :
  forall x l, is_sorted l = true -> is_sorted (insert x l) = true.
Proof.
  intros x l Hsort. destruct l as [| y rest].
  - simpl. reflexivity.
  - simpl insert. destruct (ordb x y) eqn:E.
    + (* x :: y :: rest *)
      simpl.
      assert (Hxy : leb_ord x y = true).
      { unfold leb_ord. apply negb_true_iff. apply ordb_false_iff.
        apply ord_asym. apply (ordb_true_iff x y). exact E. }
      rewrite Hxy. simpl. exact Hsort.
    + (* y :: insert x rest, use insert_hd with a := y *)
      assert (Hyx : leb_ord y x = true).
      { unfold leb_ord. apply negb_true_iff. exact E. }
      apply (insert_hd x y rest Hyx Hsort).
Qed.

Theorem sort_sorted : forall l, is_sorted (sort l) = true.
Proof.
  induction l as [| x rest IH]; simpl.
  - reflexivity.
  - apply insert_sorted. exact IH.
Qed.

(* ===========================================================================
   Section 4 - Uniqueness: within a distinct-hash list, the hash keys an entry,
   the sorted head is the minimum, and a sorted permutation is unique.
   =========================================================================== *)

(* Within a list whose hashes are distinct, the hash determines the entry. *)
Lemma hash_key_unique :
  forall l a b, NoDup (map ehash l) -> In a l -> In b l -> ehash a = ehash b -> a = b.
Proof.
  induction l as [| x rest IH]; intros a b Hnd Ha Hb Heq; simpl in *.
  - contradiction.
  - inversion Hnd as [| h t Hnotin Hnd' [Ex Et]]; subst.
    destruct Ha as [Ha | Ha]; destruct Hb as [Hb | Hb].
    + subst; reflexivity.
    + subst a. exfalso. apply Hnotin. rewrite Heq. apply in_map. exact Hb.
    + subst b. exfalso. apply Hnotin. rewrite <- Heq. apply in_map. exact Ha.
    + apply IH; assumption.
Qed.

(* In a sorted, distinct-hash list, the head is <= every later element. *)
Lemma sorted_head_min :
  forall t a, is_sorted (a :: t) = true -> NoDup (map ehash (a :: t)) ->
    Forall (fun b => leb_ord a b = true) t.
Proof.
  induction t as [| c t' IH]; intros a Hsort Hnd.
  - apply Forall_nil.
  - assert (Hac : leb_ord a c = true).
    { simpl in Hsort. apply andb_true_iff in Hsort. destruct Hsort as [H _]. exact H. }
    assert (Hsct : is_sorted (c :: t') = true) by (apply is_sorted_tail in Hsort; exact Hsort).
    (* NoDup for the tail (c :: t') *)
    assert (Hndct : NoDup (map ehash (c :: t'))).
    { simpl in Hnd. inversion Hnd; subst. assumption. }
    specialize (IH c Hsct Hndct).            (* Forall (leb_ord c) t' *)
    (* hashes: a distinct from c and from every b in t' ; c distinct from b in t' *)
    simpl in Hnd. inversion Hnd as [| hh tt Hnotin_a Hnd2 [E1 E2]]; subst.
    apply Forall_cons; [exact Hac |].
    rewrite Forall_forall in IH. rewrite Forall_forall. intros b Hbin.
    assert (Hcb : leb_ord c b = true) by (apply IH; exact Hbin).
    (* distinctness facts *)
    assert (Hac_h : ehash a <> ehash c).
    { intro Hx. apply Hnotin_a. simpl. left. symmetry. exact Hx. }
    assert (Hab_h : ehash a <> ehash b).
    { intro Hx. apply Hnotin_a. simpl. right. rewrite Hx. apply in_map. exact Hbin. }
    assert (Hcb_h : ehash c <> ehash b).
    { simpl in Hnd2. inversion Hnd2 as [| h2 t2 Hnotin_c Hnd3 [F1 F2]]; subst.
      intro Hx. apply Hnotin_c. rewrite Hx. apply in_map. exact Hbin. }
    apply (leb_ord_trans_distinct a c b Hac_h Hcb_h Hab_h Hac Hcb).
Qed.

(* THE determinism kernel: two sorted permutations with distinct hashes are EQUAL. *)
Lemma sorted_perm_nodup_eq :
  forall x y,
    NoDup (map ehash x) ->
    is_sorted x = true -> is_sorted y = true ->
    Permutation x y -> x = y.
Proof.
  induction x as [| a t IH]; intros y Hnd Hsx Hsy Hperm.
  - apply Permutation_nil in Hperm. symmetry; exact Hperm.
  - (* y is nonempty *)
    destruct y as [| b s].
    { exfalso. apply (Permutation_nil_cons (Permutation_sym Hperm)). }
    (* NoDup for y from NoDup x + permutation of the hashes *)
    assert (HndY : NoDup (map ehash (b :: s))).
    { apply (Permutation_NoDup (l := map ehash (a :: t))).
      - apply Permutation_map. exact Hperm.
      - exact Hnd. }
    (* a = b *)
    assert (Hab : a = b).
    { destruct (Nat.eq_dec (ehash a) (ehash b)) as [Heq | Hne].
      - (* same hash: both are In the (distinct-hash) list b::s -> equal *)
        apply (hash_key_unique (b :: s)).
        + exact HndY.
        + apply (Permutation_in a Hperm). left; reflexivity.
        + left; reflexivity.
        + exact Heq.
      - (* distinct hashes: a <= b (a heads x) and b <= a (b heads y) -> a = b *)
        assert (Hain : In a (b :: s)) by (apply (Permutation_in a Hperm); left; reflexivity).
        assert (Hbin : In b (a :: t)) by (apply (Permutation_in b (Permutation_sym Hperm)); left; reflexivity).
        (* a <= b *)
        assert (Haleb : leb_ord a b = true).
        { destruct Hbin as [Hbin | Hbin].
          - subst b. exfalso. apply Hne. reflexivity.
          - pose proof (sorted_head_min t a Hsx Hnd) as Hfa.
            rewrite Forall_forall in Hfa. apply Hfa. exact Hbin. }
        (* b <= a *)
        assert (Hblea : leb_ord b a = true).
        { destruct Hain as [Hain | Hain].
          - subst b. exfalso. apply Hne. reflexivity.
          - pose proof (sorted_head_min s b Hsy HndY) as Hfb.
            rewrite Forall_forall in Hfb. apply Hfb. exact Hain. }
        apply (leb_antisym a b Haleb Hblea). }
    subst b.
    (* strip heads: Permutation t s, both sorted, NoDup t *)
    assert (Hperm' : Permutation t s) by (apply (Permutation_cons_inv Hperm)).
    assert (Hst : is_sorted t = true) by (apply is_sorted_tail in Hsx; exact Hsx).
    assert (Hss : is_sorted s = true) by (apply is_sorted_tail in Hsy; exact Hsy).
    assert (Hndt : NoDup (map ehash t)).
    { simpl in Hnd. inversion Hnd; subst. assumption. }
    f_equal. apply (IH s Hndt Hst Hss Hperm').
Qed.

(* THE S1 DETERMINISM LEMMA: with distinct hashes, permuted inputs sort to the
   IDENTICAL list. Every honest node's ranked tips are byte-identical regardless
   of candidate-enumeration order -> no fork. *)
Theorem output_indep_of_input_perm :
  forall l l', NoDup (map ehash l) -> Permutation l l' -> sort l = sort l'.
Proof.
  intros l l' Hnd Hperm.
  apply sorted_perm_nodup_eq.
  - (* NoDup (map ehash (sort l)) *)
    apply (Permutation_NoDup (l := map ehash l)).
    + apply Permutation_map. apply Permutation_sym. apply sort_is_permutation.
    + exact Hnd.
  - apply sort_sorted.
  - apply sort_sorted.
  - (* Permutation (sort l) (sort l') *)
    eapply Permutation_trans; [apply sort_is_permutation |].
    eapply Permutation_trans; [exact Hperm |].
    apply Permutation_sym. apply sort_is_permutation.
Qed.

(* ===========================================================================
   Section 5 - The head is the unique maximum (the fork-choice winner).
   =========================================================================== *)

Theorem sort_argmax_unique :
  forall l, NoDup (map ehash l) ->
    match sort l with
    | [] => l = []
    | w :: _ => In w l /\ forall e, In e l -> e = w \/ ord w e
    end.
Proof.
  intros l Hnd. destruct (sort l) as [| w s] eqn:Es.
  - (* sort l = [] -> l = [] *)
    apply Permutation_nil. rewrite <- Es. apply sort_is_permutation.
  - (* w heads the sorted list *)
    assert (Hperm : Permutation (sort l) l) by apply sort_is_permutation.
    rewrite Es in Hperm.                       (* Permutation (w :: s) l *)
    assert (HndS : NoDup (map ehash (w :: s))).
    { apply (Permutation_NoDup (l := map ehash l)).
      - apply Permutation_map. apply Permutation_sym. rewrite <- Es. apply sort_is_permutation.
      - exact Hnd. }
    assert (Hsorted : is_sorted (w :: s) = true) by (rewrite <- Es; apply sort_sorted).
    split.
    + apply (Permutation_in w Hperm). left; reflexivity.
    + intros e Hin.
      (* e is in w :: s *)
      assert (HinWS : In e (w :: s)) by (apply (Permutation_in e (Permutation_sym Hperm)); exact Hin).
      destruct HinWS as [He | He].
      * left. symmetry; exact He.
      * (* e in s : w <= e, and distinct hash or equal -> e = w or ord w e *)
        pose proof (sorted_head_min s w Hsorted HndS) as Hfa.
        rewrite Forall_forall in Hfa.
        assert (Hwe : leb_ord w e = true) by (apply Hfa; exact He).
        apply (leb_ord_true_iff w e) in Hwe.   (* ~ ord e w *)
        destruct (Nat.eq_dec (ehash w) (ehash e)) as [Heq | Hne].
        -- (* same hash -> e = w (unique key in w::s) *)
           left. symmetry.
           apply (hash_key_unique (w :: s)); [exact HndS | left; reflexivity | right; exact He | exact Heq].
        -- (* distinct -> ord w e or ord e w; ~ord e w -> ord w e *)
           right. destruct (ord_total w e Hne) as [H | H]; [exact H | contradiction].
Qed.
