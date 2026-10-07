From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import LexicographicMinimax EligibleFundingAssignment FundingBox FundingOptimalDomain.
Import ListNotations.

Definition funding_contributions := nat -> nat.

Lemma funding_sum_bounds_each_entry : forall count amount source,
  source < count -> amount source <= funding_sum count amount.
Proof.
  induction count; intros amount source inside; simpl; [lia|].
  destruct (Nat.eq_dec source count) as [->|different]; [lia|].
  pose proof (IHcount amount source ltac:(lia)). lia.
Qed.

Theorem funding_coordinate_is_bounded_by_total : forall sources obligations eligible capacity demand flow source,
  assignment_valid sources obligations eligible capacity demand flow ->
  source < sources -> source_draw obligations flow source <= funding_sum obligations demand.
Proof.
  intros sources obligations eligible capacity demand flow source valid inside.
  rewrite <- (accepted_assignment_conserves_obligation _ _ _ _ _ _ valid).
  now apply funding_sum_bounds_each_entry.
Qed.

Fixpoint prefix_maximal_funding order (feasible : funding_contributions -> Prop) candidate :=
  match order with
  | [] => True
  | source :: rest =>
    (forall alternative, feasible alternative -> alternative source <= candidate source) /\
    prefix_maximal_funding rest
      (fun alternative => feasible alternative /\ alternative source = candidate source) candidate
  end.

Theorem prefix_maximal_funding_is_lexicographically_greatest : forall order feasible candidate,
  prefix_maximal_funding order feasible candidate ->
  forall alternative, feasible alternative ->
    funding_lex_le (map alternative order) (map candidate order).
Proof.
  induction order as [|source rest IH]; intros feasible candidate maximal alternative included; simpl in *; [trivial|].
  destruct maximal as [head tail]. specialize (head alternative included).
  destruct (Nat.lt_ge_cases (alternative source) (candidate source)).
  - now left.
  - right. assert (equal : alternative source = candidate source) by lia.
    split; [exact equal|]. apply (IH _ candidate tail alternative). auto.
Qed.

Theorem successor_exclusion_proves_coordinate_maximum : forall (feasible : funding_contributions -> Prop) (candidate : funding_contributions) source,
  ~ (exists alternative, feasible alternative /\ S (candidate source) <= alternative source) ->
  forall alternative, feasible alternative -> alternative source <= candidate source.
Proof.
  intros feasible candidate source excluded alternative included.
  destruct (Nat.le_gt_cases (alternative source) (candidate source)); [assumption|].
  exfalso. apply excluded. exists alternative. split; [exact included|lia].
Qed.

Theorem captured_upper_bound_proves_coordinate_maximum : forall (feasible : funding_contributions -> Prop) (candidate : funding_contributions) source bound,
  (forall alternative, feasible alternative -> alternative source <= bound) ->
  candidate source = bound ->
  forall alternative, feasible alternative -> alternative source <= candidate source.
Proof. intros. rewrite H0. now apply H. Qed.

Definition cyclic_funding_order count cursor := skipn cursor (seq 0 count) ++ firstn cursor (seq 0 count).

Theorem cyclic_funding_order_is_a_permutation : forall count cursor,
  Permutation (seq 0 count) (cyclic_funding_order count cursor).
Proof.
  intros count cursor. unfold cyclic_funding_order.
  rewrite <- (firstn_skipn cursor (seq 0 count)) at 1. apply Permutation_app_comm.
Qed.

Theorem cyclic_funding_order_has_exact_length : forall count cursor,
  length (cyclic_funding_order count cursor) = count.
Proof.
  intros count cursor.
  pose proof (Permutation_length (cyclic_funding_order_is_a_permutation count cursor)).
  rewrite length_seq in H. lia.
Qed.

Theorem cyclic_funding_order_visits_each_source_once : forall count cursor,
  NoDup (cyclic_funding_order count cursor) /\
  forall source, In source (cyclic_funding_order count cursor) <-> source < count.
Proof.
  intros count cursor. split.
  - eapply Permutation_NoDup; [apply cyclic_funding_order_is_a_permutation|apply seq_NoDup].
  - intros source. split.
    + intro included. assert (In source (seq 0 count)).
      { eapply Permutation_in; [apply Permutation_sym; apply cyclic_funding_order_is_a_permutation|exact included]. }
      apply in_seq in H. lia.
    + intro inside. eapply Permutation_in; [apply cyclic_funding_order_is_a_permutation|].
      apply in_seq. lia.
Qed.

Theorem greatest_cyclic_funding_vector_is_unique : forall count cursor left right,
  funding_lex_le (map left (cyclic_funding_order count cursor)) (map right (cyclic_funding_order count cursor)) ->
  funding_lex_le (map right (cyclic_funding_order count cursor)) (map left (cyclic_funding_order count cursor)) ->
  forall source, source < count -> left source = right source.
Proof.
  intros count cursor left right lr rl source inside.
  pose proof (funding_lex_antisymmetric _ _ lr rl) as same.
  assert (included : In source (cyclic_funding_order count cursor)).
  { apply (proj2 (cyclic_funding_order_visits_each_source_once count cursor) source). exact inside. }
  revert same included. generalize (cyclic_funding_order count cursor) as order.
  intros order. induction order as [|head rest IH]; simpl; intros same included; [contradiction|].
  inversion same. destruct included as [<-|tail]; [assumption|]. apply IH; assumption.
Qed.

Definition fix_funding_coordinate spec source amount := {|
  box_lower := fun i => if Nat.eqb i source then amount else box_lower spec i;
  box_upper := fun i => if Nat.eqb i source then amount else box_upper spec i;
  box_edges := box_edges spec
|}.

Definition raise_funding_coordinate spec source amount := {|
  box_lower := fun i => if Nat.eqb i source then S amount else box_lower spec i;
  box_upper := box_upper spec;
  box_edges := box_edges spec
|}.

Theorem fix_funding_coordinate_preserves_matching_assignment : forall sources obligations demand spec flow source amount,
  funding_spec_valid sources obligations demand spec flow ->
  source_draw obligations flow source = amount ->
  funding_spec_valid sources obligations demand (fix_funding_coordinate spec source amount) flow.
Proof.
  intros sources obligations demand spec flow source amount [[rows columns] lower] same.
  split.
  - split; [|exact columns]. intros i inside. destruct (rows i inside) as [bound edges].
    split; [|exact edges]. simpl. destruct (Nat.eqb i source) eqn:equal.
    + apply Nat.eqb_eq in equal. subst i. lia.
    + exact bound.
  - intros i inside. simpl. destruct (Nat.eqb i source) eqn:equal.
    + apply Nat.eqb_eq in equal. subst i. lia.
    + apply lower. exact inside.
Qed.

Theorem raised_funding_lower_preserves_improving_assignment : forall sources obligations demand spec flow source amount,
  funding_spec_valid sources obligations demand spec flow ->
  S amount <= source_draw obligations flow source ->
  funding_spec_valid sources obligations demand (raise_funding_coordinate spec source amount) flow.
Proof.
  intros sources obligations demand spec flow source amount [valid lower] improved.
  split; [exact valid|]. intros i inside. simpl. destruct (Nat.eqb i source) eqn:equal.
  - apply Nat.eqb_eq in equal. now subst i.
  - apply lower. exact inside.
Qed.

Definition funding_coordinate_exclusion sources obligations demand spec candidate source :=
  source_draw obligations candidate source = box_upper spec source \/
  exists selected, lower_funding_cut_check sources obligations (box_edges spec)
    (box_lower (raise_funding_coordinate spec source (source_draw obligations candidate source))) demand selected = true.

Theorem funding_coordinate_exclusion_proves_maximum : forall sources obligations demand spec candidate source,
  source < sources ->
  funding_coordinate_exclusion sources obligations demand spec candidate source ->
  forall alternative, funding_spec_valid sources obligations demand spec alternative ->
    source_draw obligations alternative source <= source_draw obligations candidate source.
Proof.
  intros sources obligations demand spec candidate source inside [capacity|[selected cut]] alternative valid.
  - rewrite capacity. exact (proj1 (proj1 (proj1 valid) source inside)).
  - destruct (Nat.le_gt_cases (source_draw obligations alternative source) (source_draw obligations candidate source)); [assumption|].
    exfalso. apply (lower_funding_cut_rejects_every_bounded_assignment sources obligations
      (box_edges spec) (box_lower (raise_funding_coordinate spec source (source_draw obligations candidate source)))
      (box_upper spec) demand selected cut).
    exists alternative. apply raised_funding_lower_preserves_improving_assignment; [exact valid|lia].
Qed.

Fixpoint funding_box_prefix_certificate sources obligations demand spec candidate order :=
  match order with
  | [] => True
  | source :: rest => source < sources /\
    funding_coordinate_exclusion sources obligations demand spec candidate source /\
    funding_box_prefix_certificate sources obligations demand
      (fix_funding_coordinate spec source (source_draw obligations candidate source)) candidate rest
  end.

Theorem checked_box_prefix_is_lexicographically_greatest : forall order sources obligations demand spec candidate,
  funding_spec_valid sources obligations demand spec candidate ->
  funding_box_prefix_certificate sources obligations demand spec candidate order ->
  forall alternative, funding_spec_valid sources obligations demand spec alternative ->
    funding_lex_le (map (source_draw obligations alternative) order) (map (source_draw obligations candidate) order).
Proof.
  induction order as [|source rest IH]; intros sources obligations demand spec candidate candidate_valid certificate alternative alternative_valid;
    simpl in *; [trivial|].
  destruct certificate as [inside [excluded tail]].
  pose proof (funding_coordinate_exclusion_proves_maximum _ _ _ _ _ _ inside excluded alternative alternative_valid) as bound.
  destruct (Nat.lt_ge_cases (source_draw obligations alternative source) (source_draw obligations candidate source)).
  - now left.
  - right. assert (same : source_draw obligations alternative source = source_draw obligations candidate source) by lia.
    split; [exact same|].
    apply (IH sources obligations demand (fix_funding_coordinate spec source (source_draw obligations candidate source)) candidate).
    + apply fix_funding_coordinate_preserves_matching_assignment; [exact candidate_valid|reflexivity].
    + exact tail.
    + apply fix_funding_coordinate_preserves_matching_assignment; assumption.
Qed.

Theorem checked_cyclic_box_certificates_have_unique_contributions : forall sources obligations demand spec cursor left right,
  funding_spec_valid sources obligations demand spec left ->
  funding_spec_valid sources obligations demand spec right ->
  funding_box_prefix_certificate sources obligations demand spec left (cyclic_funding_order sources cursor) ->
  funding_box_prefix_certificate sources obligations demand spec right (cyclic_funding_order sources cursor) ->
  forall source, source < sources -> source_draw obligations left source = source_draw obligations right source.
Proof.
  intros sources obligations demand spec cursor left right lv rv lc rc.
  apply (greatest_cyclic_funding_vector_is_unique sources cursor).
  - apply (checked_box_prefix_is_lexicographically_greatest _ _ _ _ _ right rv rc left lv).
  - apply (checked_box_prefix_is_lexicographically_greatest _ _ _ _ _ left lv lc right rv).
Qed.
