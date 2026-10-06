(* I1 (epic 8946, B1; decision record DR-75): RSpace candidate order from
   precomputed source hashes.

   Each candidate has three fields:
   - source: the precomputed hash of its source event (Produce.hash or
     Consume.hash);
   - digest: the full canonical digest of the candidate (Blake2b-256 of its
     bincode encoding), a pure function of the candidate;
   - index: its position in the store read.

   The canonical order is lexicographic in (source, digest, index). The
   implementation computes it in two phases:
   1. sort by (source, index), which reads only the precomputed hashes;
   2. re-sort each maximal run of equal sources by (digest, index). The
      implementation computes digests only for the members of runs that have
      two or more candidates (tie runs).

   Results:
   - canonical_sort_permutation, canonical_sort_sorted: the canonical order is
     a sorted permutation of its input.
   - sorted_permutation_unique: with distinct indices, at most one sorted
     arrangement exists.
   - two_phase_is_canonical, lazy_two_phase_is_canonical: the two phases (with
     or without the tie-run shortcut) produce exactly the canonical order.
   - phase_one_ignores_digests: phase one reads no digest.
   - digests_only_for_ties, two_phase_digests_only_for_ties: a digest is
     computed only for a candidate whose source hash occurs at least twice.
   - distinct_sources_need_no_digest, two_phase_distinct_sources_need_no_digest:
     with pairwise distinct source hashes, no digest is computed.
   - candidate_order_insertion_independent: the canonical payload order does
     not depend on the store's insertion order.
   - filter_commutes_with_canonical_sort: filtering before or after the sort
     selects the same candidates in the same order.
   - index_only_key_is_insertion_dependent, source_only_key_is_insertion_dependent:
     proved counterexamples show that neither weaker key is sufficient.

   The payload (source, digest) stands for the candidate value. The claim
   that the order is a function of the candidate multiset therefore relies
   on digest collision resistance, which is outside this model.

   Rust correspondence: rspace++/src/rspace/candidate_order.rs (the shared
   two-phase function), used by rspace++/src/rspace/rspace/ops_produce.rs and
   ops_consume.rs (play), rspace++/src/rspace/replay_rspace/native_candidate.rs
   (directive replay), and native_candidate/metered.rs (metered native
   replay). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

Record candidate := {
  source : nat;
  digest : nat;
  index : nat
}.

(* ------------------------------------------------------------------ *)
(* Key orders and their specifications.                                *)

Definition key_leb (a b : candidate) : bool :=
  match Nat.compare (source a) (source b) with
  | Lt => true
  | Gt => false
  | Eq =>
      match Nat.compare (digest a) (digest b) with
      | Lt => true
      | Gt => false
      | Eq => Nat.leb (index a) (index b)
      end
  end.

Definition key_le (a b : candidate) : Prop := key_leb a b = true.

Lemma key_leb_spec : forall a b,
  key_leb a b = true <->
  source a < source b \/
  (source a = source b /\
   (digest a < digest b \/ (digest a = digest b /\ index a <= index b))).
Proof.
  intros a b. unfold key_leb.
  destruct (Nat.compare_spec (source a) (source b)) as [s | s | s].
  - destruct (Nat.compare_spec (digest a) (digest b)) as [d | d | d].
    + rewrite Nat.leb_le. lia.
    + split; intros; [lia | reflexivity].
    + split; intros h; [discriminate | lia].
  - split; intros; [lia | reflexivity].
  - split; intros h; [discriminate | lia].
Qed.

Lemma key_leb_total : forall a b, key_leb a b = true \/ key_leb b a = true.
Proof. intros a b. rewrite !key_leb_spec. lia. Qed.

Lemma key_leb_trans : forall a b c,
  key_leb a b = true -> key_leb b c = true -> key_leb a c = true.
Proof. intros a b c. rewrite !key_leb_spec. lia. Qed.

Lemma key_leb_refl : forall a, key_leb a a = true.
Proof. intros a. rewrite key_leb_spec. lia. Qed.

Lemma key_leb_antisym : forall a b,
  index a <> index b \/ a = b ->
  key_leb a b = true -> key_leb b a = true -> a = b.
Proof.
  intros [sa da ia] [sb db ib] distinct ab ba.
  rewrite key_leb_spec in ab, ba. simpl in *.
  destruct distinct as [distinct | same]; [| exact same].
  exfalso. lia.
Qed.

Definition source_index_leb (a b : candidate) : bool :=
  match Nat.compare (source a) (source b) with
  | Lt => true
  | Gt => false
  | Eq => Nat.leb (index a) (index b)
  end.

Lemma source_index_leb_spec : forall a b,
  source_index_leb a b = true <->
  source a < source b \/ (source a = source b /\ index a <= index b).
Proof.
  intros a b. unfold source_index_leb.
  destruct (Nat.compare_spec (source a) (source b)) as [s | s | s].
  - rewrite Nat.leb_le. lia.
  - split; intros; [lia | reflexivity].
  - split; intros h; [discriminate | lia].
Qed.

Lemma source_index_leb_total : forall a b,
  source_index_leb a b = true \/ source_index_leb b a = true.
Proof. intros a b. rewrite !source_index_leb_spec. lia. Qed.

Definition pair_leb (a b : nat * nat) : bool :=
  match Nat.compare (fst a) (fst b) with
  | Lt => true
  | Gt => false
  | Eq => Nat.leb (snd a) (snd b)
  end.

Definition pair_le (a b : nat * nat) : Prop := pair_leb a b = true.

Lemma pair_leb_spec : forall a b,
  pair_leb a b = true <-> fst a < fst b \/ (fst a = fst b /\ snd a <= snd b).
Proof.
  intros a b. unfold pair_leb.
  destruct (Nat.compare_spec (fst a) (fst b)) as [s | s | s].
  - rewrite Nat.leb_le. lia.
  - split; intros; [lia | reflexivity].
  - split; intros h; [discriminate | lia].
Qed.

Lemma pair_leb_trans : forall a b c,
  pair_leb a b = true -> pair_leb b c = true -> pair_leb a c = true.
Proof. intros a b c. rewrite !pair_leb_spec. lia. Qed.

Lemma pair_leb_refl : forall a, pair_leb a a = true.
Proof. intros a. rewrite pair_leb_spec. lia. Qed.

Lemma pair_leb_antisym : forall a b,
  pair_leb a b = true -> pair_leb b a = true -> a = b.
Proof.
  intros [a1 a2] [b1 b2] ab ba. rewrite pair_leb_spec in ab, ba. simpl in *.
  f_equal; lia.
Qed.

(* ------------------------------------------------------------------ *)
(* Insertion sort for an arbitrary total boolean order.                *)

Section Sort.
  Context {T : Type}.
  Variable leb : T -> T -> bool.
  Hypothesis leb_total : forall a b, leb a b = true \/ leb b a = true.

  Fixpoint insert (c : T) (l : list T) : list T :=
    match l with
    | [] => [c]
    | head :: tail => if leb c head then c :: head :: tail else head :: insert c tail
    end.

  Fixpoint sort (l : list T) : list T :=
    match l with
    | [] => []
    | head :: tail => insert head (sort tail)
    end.

  Lemma insert_permutation : forall c l, Permutation (c :: l) (insert c l).
  Proof.
    intros c l. induction l as [| head tail IH]; simpl; [reflexivity |].
    destruct (leb c head); [reflexivity |].
    transitivity (head :: c :: tail); [apply perm_swap | constructor; exact IH].
  Qed.

  Lemma sort_permutation : forall l, Permutation l (sort l).
  Proof.
    induction l as [| head tail IH]; simpl; [reflexivity |].
    transitivity (head :: sort tail); [constructor; exact IH | apply insert_permutation].
  Qed.

  Lemma insert_sorted : forall c l,
    Sorted (fun a b => leb a b = true) l ->
    Sorted (fun a b => leb a b = true) (insert c l).
  Proof.
    intros c l sorted. induction sorted as [| head tail sorted IH relation]; simpl.
    - repeat constructor.
    - destruct (leb c head) eqn:ch.
      + constructor; [constructor; assumption | constructor; exact ch].
      + assert (hc : leb head c = true)
          by (destruct (leb_total c head) as [cb | bc]; [congruence | exact bc]).
        constructor; [exact IH |].
        destruct tail as [| next rest]; simpl; [constructor; exact hc |].
        inversion relation as [| ? ? hn]; subst.
        destruct (leb c next); constructor; assumption.
  Qed.

  Lemma sort_sorted : forall l, Sorted (fun a b => leb a b = true) (sort l).
  Proof.
    induction l as [| head tail IH]; simpl; [constructor | apply insert_sorted; exact IH].
  Qed.
End Sort.

(* Sorting commutes with a projection that preserves the comparison. *)
Lemma insert_map : forall (T U : Type) (leb : T -> T -> bool) (leb' : U -> U -> bool) (f : T -> U),
  (forall a b, leb a b = leb' (f a) (f b)) ->
  forall c l, map f (insert leb c l) = insert leb' (f c) (map f l).
Proof.
  intros T U leb leb' f same c l. induction l as [| head tail IH]; simpl; [reflexivity |].
  rewrite same. destruct (leb' (f c) (f head)); simpl; [reflexivity | rewrite IH; reflexivity].
Qed.

Lemma sort_map : forall (T U : Type) (leb : T -> T -> bool) (leb' : U -> U -> bool) (f : T -> U),
  (forall a b, leb a b = leb' (f a) (f b)) ->
  forall l, map f (sort leb l) = sort leb' (map f l).
Proof.
  intros T U leb leb' f same l. induction l as [| head tail IH]; simpl; [reflexivity |].
  rewrite (insert_map T U leb leb' f same), IH. reflexivity.
Qed.

Definition canonical_sort := sort key_leb.

Theorem canonical_sort_permutation : forall l, Permutation l (canonical_sort l).
Proof. apply sort_permutation. Qed.

Theorem canonical_sort_sorted : forall l, Sorted key_le (canonical_sort l).
Proof. intros l. apply (sort_sorted key_leb key_leb_total). Qed.

(* ------------------------------------------------------------------ *)
(* Uniqueness of the sorted arrangement.                               *)

Theorem sorted_permutation_unique : forall l1 l2,
  NoDup (map index l1) ->
  Sorted key_le l1 -> Sorted key_le l2 -> Permutation l1 l2 -> l1 = l2.
Proof.
  induction l1 as [| a rest IH]; intros l2 distinct sorted1 sorted2 perm.
  - symmetry. apply Permutation_nil. exact perm.
  - destruct l2 as [| b rest2].
    + apply Permutation_sym in perm. apply Permutation_nil_cons in perm. contradiction.
    + assert (strongly1 : StronglySorted key_le (a :: rest))
        by (apply Sorted_StronglySorted; [intros x y z; apply key_leb_trans | exact sorted1]).
      assert (strongly2 : StronglySorted key_le (b :: rest2))
        by (apply Sorted_StronglySorted; [intros x y z; apply key_leb_trans | exact sorted2]).
      assert (in_b : In a (b :: rest2)) by (apply (Permutation_in a perm); left; reflexivity).
      assert (in_a : In b (a :: rest))
        by (apply (Permutation_in b (Permutation_sym perm)); left; reflexivity).
      inversion strongly1 as [| ? ? _ above_a]; subst.
      inversion strongly2 as [| ? ? _ above_b]; subst.
      rewrite Forall_forall in above_a, above_b.
      assert (ab : key_leb a b = true)
        by (destruct in_a as [same | later]; [subst; apply key_leb_refl | exact (above_a b later)]).
      assert (ba : key_leb b a = true)
        by (destruct in_b as [same | later]; [subst; apply key_leb_refl | exact (above_b a later)]).
      assert (head_equal : a = b).
      { apply key_leb_antisym; [| exact ab | exact ba].
        destruct (Nat.eq_dec (index a) (index b)) as [same_index | different];
          [| left; exact different].
        right.
        destruct in_a as [same | later]; [exact same |].
        exfalso. simpl in distinct. inversion distinct as [| ? ? not_in _]; subst.
        apply not_in. rewrite same_index. apply in_map. exact later. }
      subst b. f_equal.
      apply IH.
      * simpl in distinct. inversion distinct; assumption.
      * inversion sorted1; assumption.
      * inversion sorted2; assumption.
      * apply Permutation_cons_inv with a. exact perm.
Qed.

(* ------------------------------------------------------------------ *)
(* Maximal runs of equal source.                                       *)

Fixpoint runs (l : list candidate) : list (list candidate) :=
  match l with
  | [] => []
  | head :: tail =>
      match runs tail with
      | (first :: rest_run) :: others =>
          if Nat.eqb (source head) (source first)
          then (head :: first :: rest_run) :: others
          else [head] :: (first :: rest_run) :: others
      | other => [head] :: other
      end
  end.

Definition run_head (run : list candidate) : nat :=
  match run with
  | c :: _ => source c
  | [] => 0
  end.

Definition well_formed_run (run : list candidate) : Prop :=
  run <> [] /\ Forall (fun c => source c = run_head run) run.

Lemma runs_well_formed : forall l, Forall well_formed_run (runs l).
Proof.
  induction l as [| head tail IH]; simpl; [constructor |].
  destruct (runs tail) as [| [| first rest_run] others] eqn:tail_runs.
  - constructor; [split; [discriminate | constructor; [reflexivity | constructor]] | constructor].
  - inversion IH as [| ? ? [nonempty _] _]. exfalso. apply nonempty. reflexivity.
  - inversion IH as [| ? ? [_ first_constant] others_ok]; subst.
    destruct (Nat.eqb (source head) (source first)) eqn:same.
    + apply Nat.eqb_eq in same.
      constructor; [| exact others_ok].
      split; [discriminate |].
      simpl in first_constant |- *.
      constructor; [reflexivity |].
      eapply Forall_impl; [| exact first_constant].
      intros c c_same. simpl in c_same. lia.
    + constructor; [split; [discriminate | constructor; [reflexivity | constructor]] |].
      constructor; [split; [discriminate | exact first_constant] | exact others_ok].
Qed.

Lemma runs_concat : forall l, concat (runs l) = l.
Proof.
  induction l as [| head tail IH]; simpl; [reflexivity |].
  destruct (runs tail) as [| [| first rest_run] others] eqn:tail_runs.
  - simpl in IH. subst tail. reflexivity.
  - simpl in IH |- *. rewrite IH. reflexivity.
  - destruct (Nat.eqb (source head) (source first)); simpl in IH |- *; rewrite IH; reflexivity.
Qed.

Definition source_le (a b : candidate) : Prop := source a <= source b.

Lemma runs_heads_increasing : forall l,
  Sorted source_le l -> Sorted lt (map run_head (runs l)).
Proof.
  induction l as [| head tail IH]; intros sorted; simpl; [constructor |].
  inversion sorted as [| ? ? tail_sorted relation]; subst.
  specialize (IH tail_sorted).
  pose proof (runs_concat tail) as tail_concat.
  pose proof (runs_well_formed tail) as tail_shape.
  destruct (runs tail) as [| [| first rest_run] others] eqn:tail_runs; simpl in *.
  - repeat constructor.
  - inversion tail_shape as [| ? ? [nonempty _] _]. exfalso. apply nonempty. reflexivity.
  - destruct (Nat.eqb (source head) (source first)) eqn:same; simpl.
    + apply Nat.eqb_eq in same. rewrite same. exact IH.
    + apply Nat.eqb_neq in same.
      constructor; [exact IH |].
      constructor.
      rewrite <- tail_concat in relation.
      inversion relation as [| ? ? le]; subst. unfold source_le in le. lia.
Qed.

(* ------------------------------------------------------------------ *)
(* The two-phase algorithm.                                            *)

Definition phase_one := sort source_index_leb.

Definition phase_two (l : list candidate) : list candidate :=
  concat (map canonical_sort (runs l)).

Definition two_phase_order (l : list candidate) : list candidate :=
  phase_two (phase_one l).

Lemma phase_one_source_sorted : forall l, Sorted source_le (phase_one l).
Proof.
  intros l.
  pose proof (sort_sorted source_index_leb source_index_leb_total l) as sorted.
  unfold phase_one.
  induction sorted as [| a rest _ IH relation]; constructor; [exact IH |].
  inversion relation as [| b more le]; subst; constructor.
  apply source_index_leb_spec in le. unfold source_le. lia.
Qed.

Lemma strongly_sorted_app : forall (R : candidate -> candidate -> Prop) l1 l2,
  StronglySorted R l1 -> StronglySorted R l2 ->
  (forall x y, In x l1 -> In y l2 -> R x y) ->
  StronglySorted R (l1 ++ l2).
Proof.
  intros R l1 l2 sorted1 sorted2 across.
  induction sorted1 as [| a rest _ IH above]; simpl; [exact sorted2 |].
  constructor.
  - apply IH. intros x y in_x in_y. apply across; [right; exact in_x | exact in_y].
  - rewrite Forall_forall in above |- *. intros y in_y.
    apply in_app_or in in_y as [in_rest | in_l2].
    + exact (above y in_rest).
    + apply across; [left; reflexivity | exact in_l2].
Qed.

Lemma concat_sorted_runs : forall rs,
  Forall well_formed_run rs ->
  Sorted lt (map run_head rs) ->
  Sorted key_le (concat (map canonical_sort rs)).
Proof.
  induction rs as [| run more IH]; intros shape increasing; simpl; [constructor |].
  inversion shape as [| ? ? [nonempty same] rest_shape]; subst.
  assert (strongly : StronglySorted lt (map run_head (run :: more)))
    by (apply Sorted_StronglySorted; [intros x y z; lia | exact increasing]).
  inversion strongly as [| ? ? _ below]; subst.
  assert (later_sorted : Sorted key_le (concat (map canonical_sort more)))
    by (apply IH; [exact rest_shape | inversion increasing; assumption]).
  assert (above : forall later, In later (concat (map canonical_sort more)) ->
                   run_head run < source later).
  { intros later in_later.
    apply in_concat in in_later as [sorted_run [in_runs in_run]].
    apply in_map_iff in in_runs as [run' [sorted_eq in_more]]. subst sorted_run.
    apply (Permutation_in later (Permutation_sym (canonical_sort_permutation run'))) in in_run.
    rewrite Forall_forall in rest_shape. destruct (rest_shape run' in_more) as [_ same'].
    rewrite Forall_forall in same'. rewrite (same' later in_run).
    rewrite Forall_forall in below. apply below. apply in_map. exact in_more. }
  assert (run_sources : forall c, In c (canonical_sort run) -> source c = run_head run).
  { intros c in_c.
    apply (Permutation_in c (Permutation_sym (canonical_sort_permutation run))) in in_c.
    rewrite Forall_forall in same. exact (same c in_c). }
  apply StronglySorted_Sorted.
  apply strongly_sorted_app.
  - apply Sorted_StronglySorted; [intros x y z; apply key_leb_trans | apply canonical_sort_sorted].
  - apply Sorted_StronglySorted; [intros x y z; apply key_leb_trans | exact later_sorted].
  - intros x y in_x in_y. unfold key_le. rewrite key_leb_spec.
    rewrite (run_sources x in_x). left. exact (above y in_y).
Qed.

Lemma phase_two_sorted : forall l, Sorted source_le l -> Sorted key_le (phase_two l).
Proof.
  intros l sorted. apply concat_sorted_runs.
  - apply runs_well_formed.
  - apply runs_heads_increasing. exact sorted.
Qed.

Lemma phase_two_permutation : forall l, Permutation l (phase_two l).
Proof.
  intros l. unfold phase_two.
  rewrite <- (runs_concat l) at 1.
  induction (runs l) as [| run more IH]; simpl; [reflexivity |].
  apply Permutation_app; [apply canonical_sort_permutation | exact IH].
Qed.

(* The two-phase algorithm produces exactly the canonical order. *)
Theorem two_phase_is_canonical : forall l,
  NoDup (map index l) -> two_phase_order l = canonical_sort l.
Proof.
  intros l distinct.
  assert (perm : Permutation (two_phase_order l) (canonical_sort l)).
  { unfold two_phase_order.
    transitivity l.
    - symmetry. transitivity (phase_one l);
        [apply sort_permutation | apply phase_two_permutation].
    - apply canonical_sort_permutation. }
  apply sorted_permutation_unique.
  - apply (Permutation_NoDup (Permutation_map index (Permutation_sym perm))).
    apply (Permutation_NoDup (Permutation_map index (canonical_sort_permutation l))).
    exact distinct.
  - unfold two_phase_order. apply phase_two_sorted. apply phase_one_source_sorted.
  - apply canonical_sort_sorted.
  - exact perm.
Qed.

(* ------------------------------------------------------------------ *)
(* Insertion independence and filtering.                               *)

Definition payload (c : candidate) : nat * nat := (source c, digest c).

Lemma pair_sorted_permutation_unique : forall l1 l2,
  Sorted pair_le l1 -> Sorted pair_le l2 -> Permutation l1 l2 -> l1 = l2.
Proof.
  induction l1 as [| a rest IH]; intros l2 sorted1 sorted2 perm.
  - symmetry. apply Permutation_nil. exact perm.
  - destruct l2 as [| b rest2].
    + apply Permutation_sym in perm. apply Permutation_nil_cons in perm. contradiction.
    + assert (strongly1 : StronglySorted pair_le (a :: rest))
        by (apply Sorted_StronglySorted; [intros x y z; apply pair_leb_trans | exact sorted1]).
      assert (strongly2 : StronglySorted pair_le (b :: rest2))
        by (apply Sorted_StronglySorted; [intros x y z; apply pair_leb_trans | exact sorted2]).
      assert (in_b : In a (b :: rest2)) by (apply (Permutation_in a perm); left; reflexivity).
      assert (in_a : In b (a :: rest))
        by (apply (Permutation_in b (Permutation_sym perm)); left; reflexivity).
      inversion strongly1 as [| ? ? _ above_a]; subst.
      inversion strongly2 as [| ? ? _ above_b]; subst.
      rewrite Forall_forall in above_a, above_b.
      assert (head_equal : a = b).
      { apply pair_leb_antisym.
        - destruct in_a as [same | later]; [subst; apply pair_leb_refl | exact (above_a b later)].
        - destruct in_b as [same | later]; [subst; apply pair_leb_refl | exact (above_b a later)]. }
      subst b. f_equal.
      apply IH.
      * inversion sorted1; assumption.
      * inversion sorted2; assumption.
      * apply Permutation_cons_inv with a. exact perm.
Qed.

Lemma sorted_map_payload : forall l, Sorted key_le l -> Sorted pair_le (map payload l).
Proof.
  intros l sorted. induction sorted as [| a rest sorted IH relation]; simpl; [constructor |].
  constructor; [exact IH |].
  inversion relation as [| b more le]; subst; simpl; constructor.
  unfold pair_le, key_le in *. rewrite pair_leb_spec. rewrite key_leb_spec in le.
  unfold payload. simpl. lia.
Qed.

Fixpoint indexed_from (position : nat) (payloads : list (nat * nat)) : list candidate :=
  match payloads with
  | [] => []
  | (s, d) :: rest => Build_candidate s d position :: indexed_from (S position) rest
  end.

Definition indexed (payloads : list (nat * nat)) : list candidate := indexed_from 0 payloads.

Lemma indexed_from_payload : forall payloads position,
  map payload (indexed_from position payloads) = payloads.
Proof.
  induction payloads as [| [s d] rest IH]; intros position; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

(* The canonical payload order depends only on the multiset of payloads:
   permuting the store read does not change it. *)
Theorem candidate_order_insertion_independent : forall p1 p2,
  Permutation p1 p2 ->
  map payload (canonical_sort (indexed p1)) = map payload (canonical_sort (indexed p2)).
Proof.
  intros p1 p2 perm.
  apply pair_sorted_permutation_unique.
  - apply sorted_map_payload. apply canonical_sort_sorted.
  - apply sorted_map_payload. apply canonical_sort_sorted.
  - transitivity p1.
    + rewrite <- (indexed_from_payload p1 0) at 2.
      apply Permutation_map. apply Permutation_sym. apply canonical_sort_permutation.
    + transitivity p2; [exact perm |].
      rewrite <- (indexed_from_payload p2 0) at 1.
      apply Permutation_map. apply canonical_sort_permutation.
Qed.

Lemma filter_preserves_permutation : forall (f : candidate -> bool) l1 l2,
  Permutation l1 l2 -> Permutation (filter f l1) (filter f l2).
Proof.
  intros f l1 l2 perm.
  induction perm as [| x l1' l2' _ IH | x y l | l1' l2' l3' _ IH1 _ IH2]; simpl.
  - constructor.
  - destruct (f x); [constructor; exact IH | exact IH].
  - destruct (f x), (f y); try apply perm_swap; reflexivity.
  - transitivity (filter f l2'); assumption.
Qed.

Lemma filter_preserves_sorted : forall (f : candidate -> bool) l,
  Sorted key_le l -> Sorted key_le (filter f l).
Proof.
  intros f l sorted.
  apply StronglySorted_Sorted.
  apply Sorted_StronglySorted in sorted; [| intros x y z; apply key_leb_trans].
  induction sorted as [| a rest _ IH above]; simpl; [constructor |].
  destruct (f a); [| exact IH].
  constructor; [exact IH |].
  rewrite Forall_forall in above |- *. intros x in_x.
  apply filter_In in in_x as [in_rest _]. exact (above x in_rest).
Qed.

Lemma NoDup_map_filter : forall (f : candidate -> bool) l,
  NoDup (map index l) -> NoDup (map index (filter f l)).
Proof.
  intros f l distinct. induction l as [| a rest IH]; simpl in *; [constructor |].
  inversion distinct as [| ? ? not_in rest_distinct]; subst.
  destruct (f a); simpl; [| exact (IH rest_distinct)].
  constructor; [| exact (IH rest_distinct)].
  intro in_filtered. apply not_in.
  apply in_map_iff in in_filtered as [c [same in_c]].
  apply filter_In in in_c as [in_rest _].
  rewrite <- same. apply in_map. exact in_rest.
Qed.

(* Filtering candidates (for example by a match predicate) commutes with the
   canonical order, so play and replay see the same relative order. *)
Theorem filter_commutes_with_canonical_sort : forall (f : candidate -> bool) l,
  NoDup (map index l) ->
  filter f (canonical_sort l) = canonical_sort (filter f l).
Proof.
  intros f l distinct.
  apply sorted_permutation_unique.
  - apply (Permutation_NoDup (Permutation_map index
      (filter_preserves_permutation f _ _ (canonical_sort_permutation l)))).
    apply NoDup_map_filter. exact distinct.
  - apply filter_preserves_sorted. apply canonical_sort_sorted.
  - apply canonical_sort_sorted.
  - transitivity (filter f l).
    + apply filter_preserves_permutation. apply Permutation_sym.
      apply canonical_sort_permutation.
    + apply canonical_sort_permutation.
Qed.

(* ------------------------------------------------------------------ *)
(* Digest work: phase one reads no digest, phase two reads digests only *)
(* inside runs of two or more equal sources.                           *)

Definition strip (c : candidate) : nat * nat := (source c, index c).

(* The (source, index) sequence after phase one depends only on the
   (source, index) sequence of the input, so phase one never reads a digest. *)
Theorem phase_one_ignores_digests : forall l1 l2,
  map strip l1 = map strip l2 -> map strip (phase_one l1) = map strip (phase_one l2).
Proof.
  intros l1 l2 same.
  assert (key : forall a b, source_index_leb a b = pair_leb (strip a) (strip b))
    by (intros a b; reflexivity).
  unfold phase_one.
  rewrite (sort_map _ _ source_index_leb pair_leb strip key l1),
          (sort_map _ _ source_index_leb pair_leb strip key l2), same.
  reflexivity.
Qed.

Definition tie_run (run : list candidate) : bool := Nat.leb 2 (length run).

(* The implementation sorts a run by digest only when the run is a tie run. *)
Definition settle_run (run : list candidate) : list candidate :=
  if tie_run run then canonical_sort run else run.

Definition lazy_phase_two (l : list candidate) : list candidate :=
  concat (map settle_run (runs l)).

Definition lazy_two_phase_order (l : list candidate) : list candidate :=
  lazy_phase_two (phase_one l).

Lemma settle_run_is_canonical_sort : forall run, settle_run run = canonical_sort run.
Proof. intros [| x [| y rest]]; reflexivity. Qed.

Lemma non_tie_run_is_unchanged : forall run, tie_run run = false -> settle_run run = run.
Proof. intros run single. unfold settle_run. rewrite single. reflexivity. Qed.

Lemma lazy_phase_two_is_phase_two : forall l, lazy_phase_two l = phase_two l.
Proof.
  intros l. unfold lazy_phase_two, phase_two. f_equal.
  apply map_ext. apply settle_run_is_canonical_sort.
Qed.

(* The implemented algorithm (lazy phase two) produces the canonical order. *)
Theorem lazy_two_phase_is_canonical : forall l,
  NoDup (map index l) -> lazy_two_phase_order l = canonical_sort l.
Proof.
  intros l distinct. unfold lazy_two_phase_order.
  rewrite lazy_phase_two_is_phase_two.
  exact (two_phase_is_canonical l distinct).
Qed.

(* The candidates whose digest the implementation computes. *)
Definition digested (l : list candidate) : list candidate :=
  concat (filter tie_run (runs l)).

Definition same_source_count (s : nat) (l : list candidate) : nat :=
  length (filter (fun c => Nat.eqb (source c) s) l).

Lemma same_source_count_app : forall s l1 l2,
  same_source_count s (l1 ++ l2) = same_source_count s l1 + same_source_count s l2.
Proof. intros s l1 l2. unfold same_source_count. rewrite filter_app, length_app. reflexivity. Qed.

Lemma same_source_count_concat : forall s rs,
  same_source_count s (concat rs) = list_sum (map (same_source_count s) rs).
Proof.
  intros s rs. induction rs as [| run more IH]; simpl; [reflexivity |].
  rewrite same_source_count_app, IH. reflexivity.
Qed.

Lemma list_sum_member : forall (f : list candidate -> nat) rs run,
  In run rs -> f run <= list_sum (map f rs).
Proof.
  intros f rs run. induction rs as [| other more IH]; simpl; [contradiction |].
  intros [same | later]; [subst; lia | specialize (IH later); lia].
Qed.

Lemma uniform_run_count : forall run s,
  Forall (fun c => source c = s) run -> same_source_count s run = length run.
Proof.
  intros run s uniform. unfold same_source_count.
  induction uniform as [| c rest same _ IH]; simpl; [reflexivity |].
  rewrite same, Nat.eqb_refl. simpl. rewrite IH. reflexivity.
Qed.

Lemma same_source_count_permutation : forall s l1 l2,
  Permutation l1 l2 -> same_source_count s l1 = same_source_count s l2.
Proof.
  intros s l1 l2 perm. unfold same_source_count.
  apply Permutation_length. apply filter_preserves_permutation. exact perm.
Qed.

(* A digest is computed only for a candidate whose source hash occurs at
   least twice in the input. *)
Theorem digests_only_for_ties : forall l c,
  In c (digested l) -> 2 <= same_source_count (source c) l.
Proof.
  intros l c in_digested. unfold digested in in_digested.
  apply in_concat in in_digested as [run [in_ties in_run]].
  apply filter_In in in_ties as [in_runs tie].
  pose proof (runs_well_formed l) as shape.
  rewrite Forall_forall in shape.
  destruct (shape run in_runs) as [_ uniform].
  assert (head : source c = run_head run).
  { rewrite Forall_forall in uniform. exact (uniform c in_run). }
  rewrite <- (runs_concat l), same_source_count_concat.
  apply Nat.le_trans with (same_source_count (source c) run).
  - rewrite head, (uniform_run_count run (run_head run) uniform).
    unfold tie_run in tie. apply Nat.leb_le in tie. exact tie.
  - apply list_sum_member. exact in_runs.
Qed.

Lemma count_at_most_one_of_distinct_sources : forall l s,
  NoDup (map source l) -> same_source_count s l <= 1.
Proof.
  induction l as [| c rest IH]; intros s distinct; unfold same_source_count in *; simpl; [lia |].
  inversion distinct as [| ? ? not_in rest_distinct]; subst.
  destruct (Nat.eqb (source c) s) eqn:hit; simpl.
  - apply Nat.eqb_eq in hit. subst s.
    assert (zero : length (filter (fun d => Nat.eqb (source d) (source c)) rest) = 0).
    { destruct (filter (fun d => Nat.eqb (source d) (source c)) rest) as [| d more] eqn:found;
        [reflexivity |].
      exfalso. apply not_in.
      assert (in_d : In d (filter (fun e => Nat.eqb (source e) (source c)) rest))
        by (rewrite found; left; reflexivity).
      apply filter_In in in_d as [in_rest same].
      apply Nat.eqb_eq in same. rewrite <- same. apply in_map. exact in_rest. }
    rewrite zero. lia.
  - apply IH. exact rest_distinct.
Qed.

(* With pairwise distinct source hashes, no digest is computed at all. *)
Theorem distinct_sources_need_no_digest : forall l,
  NoDup (map source l) -> digested l = [].
Proof.
  intros l distinct.
  destruct (digested l) as [| c more] eqn:found; [reflexivity |].
  exfalso.
  assert (in_c : In c (digested l)) by (rewrite found; left; reflexivity).
  pose proof (digests_only_for_ties l c in_c) as two.
  pose proof (count_at_most_one_of_distinct_sources l (source c) distinct) as one.
  lia.
Qed.

(* The same two facts for the runs that the implementation forms after phase one. *)
Theorem two_phase_digests_only_for_ties : forall l c,
  In c (digested (phase_one l)) -> 2 <= same_source_count (source c) l.
Proof.
  intros l c in_c.
  rewrite (same_source_count_permutation (source c) l (phase_one l)
             (sort_permutation source_index_leb l)).
  apply digests_only_for_ties. exact in_c.
Qed.

Theorem two_phase_distinct_sources_need_no_digest : forall l,
  NoDup (map source l) -> digested (phase_one l) = [].
Proof.
  intros l distinct. apply distinct_sources_need_no_digest.
  apply (Permutation_NoDup (Permutation_map source (sort_permutation source_index_leb l))).
  exact distinct.
Qed.

(* ------------------------------------------------------------------ *)
(* Weaker keys are not sufficient.                                     *)

Definition index_only_order := sort (fun a b => Nat.leb (index a) (index b)).

(* Sorting by store position alone depends on the store's insertion order. *)
Theorem index_only_key_is_insertion_dependent : exists p1 p2,
  Permutation p1 p2 /\
  map payload (index_only_order (indexed p1)) <> map payload (index_only_order (indexed p2)).
Proof.
  exists [(1, 0); (2, 0)], [(2, 0); (1, 0)]. split.
  - apply perm_swap.
  - vm_compute. discriminate.
Qed.

(* Sorting by source hash alone, with the index as the tie-break, depends on
   insertion order when two distinct candidates share a source hash. *)
Theorem source_only_key_is_insertion_dependent : exists p1 p2,
  Permutation p1 p2 /\
  map payload (phase_one (indexed p1)) <> map payload (phase_one (indexed p2)).
Proof.
  exists [(1, 5); (1, 7)], [(1, 7); (1, 5)]. split.
  - apply perm_swap.
  - vm_compute. discriminate.
Qed.

Print Assumptions canonical_sort_permutation.
Print Assumptions canonical_sort_sorted.
Print Assumptions sorted_permutation_unique.
Print Assumptions two_phase_is_canonical.
Print Assumptions candidate_order_insertion_independent.
Print Assumptions filter_commutes_with_canonical_sort.
Print Assumptions index_only_key_is_insertion_dependent.
Print Assumptions source_only_key_is_insertion_dependent.
Print Assumptions phase_one_ignores_digests.
Print Assumptions lazy_two_phase_is_canonical.
Print Assumptions digests_only_for_ties.
Print Assumptions distinct_sources_need_no_digest.
Print Assumptions two_phase_digests_only_for_ties.
Print Assumptions two_phase_distinct_sources_need_no_digest.
