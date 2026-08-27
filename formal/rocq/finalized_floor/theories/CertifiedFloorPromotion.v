From Stdlib Require Import Lists.List.
Import ListNotations.

Section General.

Context {Block : Type}.

Variable dag_ancestor : Block -> Block -> Prop.
Variable certified : Block -> Prop.
Variable state_certified : Block -> Prop.
Variable preserves : Block -> Block -> Prop.

Definition causal_discovered (parents : list Block) (candidate : Block) : Prop :=
  exists parent, In parent parents /\ dag_ancestor candidate parent.

Definition universal_candidate
  (current : Block)
  (parents : list Block)
  (candidate : Block)
  : Prop :=
  certified candidate /\
  state_certified candidate /\
  preserves current candidate /\
  Forall (dag_ancestor candidate) parents.

Theorem preserving_parents_make_current_universal :
  (forall ancestor descendant,
    preserves ancestor descendant -> dag_ancestor ancestor descendant) ->
  forall current parents,
    Forall (preserves current) parents ->
    Forall (dag_ancestor current) parents.
Proof.
  intros Hpreservation current parents Hparents.
  induction Hparents as [|parent tail Hparent Htail IH].
  - constructor.
  - constructor.
    + exact (Hpreservation current parent Hparent).
    + exact IH.
Qed.

Theorem dual_certified_current_floor_is_candidate :
  (forall ancestor descendant,
    preserves ancestor descendant -> dag_ancestor ancestor descendant) ->
  (forall block, preserves block block) ->
  forall current parents,
    certified current ->
    state_certified current ->
    Forall (preserves current) parents ->
    universal_candidate current parents current.
Proof.
  intros Hpreservation Hreflexive current parents Hcertified Hstate Hparents.
  repeat split.
  - exact Hcertified.
  - exact Hstate.
  - exact (Hreflexive current).
  - exact (preserving_parents_make_current_universal
      Hpreservation current parents Hparents).
Qed.

Theorem dual_certified_current_floor_is_discoverable :
  (forall ancestor descendant,
    preserves ancestor descendant -> dag_ancestor ancestor descendant) ->
  forall current parents,
    parents <> [] ->
    certified current ->
    state_certified current ->
    Forall (preserves current) parents ->
    causal_discovered parents current.
Proof.
  intros Hpreservation current parents Hnonempty _ _ Hparents.
  destruct parents as [|parent tail].
  - contradiction.
  - inversion Hparents as [|head rest Hparent Htail]; subst.
    exists parent.
    split.
    + left. reflexivity.
    + exact (Hpreservation current parent Hparent).
Qed.

Variable select : list Block -> option Block.

Definition selected_candidate_sound
  (current : Block)
  (parents candidates : list Block)
  : Prop :=
  forall chosen,
    select candidates = Some chosen ->
    universal_candidate current parents chosen.

Theorem selected_floor_preserves_current :
  forall current parents candidates chosen,
    selected_candidate_sound current parents candidates ->
    select candidates = Some chosen ->
    preserves current chosen.
Proof.
  intros current parents candidates chosen Hsound Hselected.
  destruct (Hsound chosen Hselected) as [_ [_ [Hpreserves _]]].
  exact Hpreserves.
Qed.

End General.

Inductive scenario_block : Type :=
| Genesis
| CommittedFloor
| Left
| Right
| MergeLeft
| MergeRight.

Definition scenario_main_ancestor
  (ancestor descendant : scenario_block)
  : Prop :=
  match ancestor, descendant with
  | Genesis, _ => True
  | CommittedFloor, CommittedFloor => True
  | Left, Left => True
  | Left, MergeLeft => True
  | Right, Right => True
  | Right, MergeRight => True
  | MergeLeft, MergeLeft => True
  | MergeRight, MergeRight => True
  | _, _ => False
  end.

Definition scenario_dag_ancestor
  (ancestor descendant : scenario_block)
  : Prop :=
  scenario_main_ancestor ancestor descendant \/
  match ancestor, descendant with
  | CommittedFloor, MergeLeft => True
  | CommittedFloor, MergeRight => True
  | _, _ => False
  end.

Definition scenario_preserves := scenario_dag_ancestor.

Definition scenario_certified (block : scenario_block) : Prop :=
  block = Genesis \/ block = CommittedFloor.

Definition scenario_state_certified := scenario_certified.

Definition main_discovered
  (parents : list scenario_block)
  (candidate : scenario_block)
  : Prop :=
  exists parent,
    In parent parents /\ scenario_main_ancestor candidate parent.

Definition certified_floor_promotion_contract : Prop :=
  ~ main_discovered [MergeLeft; MergeRight] CommittedFloor /\
  causal_discovered
    scenario_dag_ancestor
    [MergeLeft; MergeRight]
    CommittedFloor /\
  universal_candidate
    scenario_dag_ancestor
    scenario_certified
    scenario_state_certified
    scenario_preserves
    CommittedFloor
    [MergeLeft; MergeRight]
    CommittedFloor.

Theorem certified_floor_promotion_end_to_end :
  certified_floor_promotion_contract.
Proof.
  unfold certified_floor_promotion_contract, main_discovered,
    causal_discovered, universal_candidate, scenario_dag_ancestor,
    scenario_main_ancestor, scenario_certified,
    scenario_state_certified, scenario_preserves.
  split.
  - intros [parent [[Heq | [Heq | Habsurd]] Hmain]].
    + subst parent. exact Hmain.
    + subst parent. exact Hmain.
    + contradiction.
  - split.
    + exists MergeLeft. simpl. tauto.
    + repeat split.
      * right. reflexivity.
      * right. reflexivity.
      * left. exact I.
      * constructor.
        -- right. exact I.
        -- constructor.
           ++ right. exact I.
           ++ constructor.
Qed.

Print Assumptions certified_floor_promotion_end_to_end.

Section CoverageOptimization.

Context {Block Validator : Type}.

Variable parent_edge : Block -> Block -> Prop.
Variable latest : Validator -> Block.

Inductive dag_reaches : Block -> Block -> Prop :=
| dag_reaches_refl : forall block, dag_reaches block block
| dag_reaches_step : forall ancestor child descendant,
    parent_edge ancestor child ->
    dag_reaches child descendant ->
    dag_reaches ancestor descendant.

Inductive propagated_coverage : Block -> Validator -> Prop :=
| propagated_coverage_seed : forall validator,
    propagated_coverage (latest validator) validator
| propagated_coverage_parent : forall parent child validator,
    parent_edge parent child ->
    propagated_coverage child validator ->
    propagated_coverage parent validator.

Definition pairwise_support
  (candidate : Block)
  (validator : Validator)
  : Prop :=
  dag_reaches candidate (latest validator).

Lemma reaches_to_propagated_coverage :
  forall ancestor descendant,
    dag_reaches ancestor descendant ->
    forall validator,
      descendant = latest validator ->
      propagated_coverage ancestor validator.
Proof.
  intros ancestor descendant Hreaches.
  induction Hreaches.
  - intros validator Heq.
    subst block.
    apply propagated_coverage_seed.
  - intros validator Heq.
    apply (propagated_coverage_parent ancestor child validator).
    + exact H.
    + apply IHHreaches. exact Heq.
Qed.

Theorem propagated_coverage_exact :
  forall candidate validator,
    propagated_coverage candidate validator <->
    pairwise_support candidate validator.
Proof.
  intros candidate validator.
  split.
  - intros Hcoverage.
    induction Hcoverage.
    + apply dag_reaches_refl.
    + apply (dag_reaches_step parent child (latest validator)); assumption.
  - intros Hreaches.
    apply (reaches_to_propagated_coverage
      candidate (latest validator) Hreaches validator).
    reflexivity.
Qed.

Theorem coverage_decision_transparent :
  forall candidate
    (decide : (Validator -> Prop) -> Prop),
    (forall left right,
      (forall validator, left validator <-> right validator) ->
      (decide left <-> decide right)) ->
    decide (fun validator => propagated_coverage candidate validator) <->
    decide (fun validator => pairwise_support candidate validator).
Proof.
  intros candidate decide Hextensional.
  apply Hextensional.
  intros validator.
  apply propagated_coverage_exact.
Qed.

Lemma reaches_last_edge :
  forall ancestor descendant,
    dag_reaches ancestor descendant ->
    ancestor = descendant \/
    exists immediate,
      dag_reaches ancestor immediate /\
      parent_edge immediate descendant.
Proof.
  intros ancestor descendant Hreaches.
  induction Hreaches.
  - left. reflexivity.
  - destruct IHHreaches as [Hchild | [immediate [Hprefix Hlast]]].
    + right.
      exists ancestor.
      split.
      * apply dag_reaches_refl.
      * rewrite <- Hchild. exact H.
    + right.
      exists immediate.
      split.
      * apply (dag_reaches_step ancestor child immediate); assumption.
      * exact Hlast.
Qed.

Theorem reaches_unique_predecessor :
  forall predecessor endpoint ancestor,
    (forall immediate,
      parent_edge immediate endpoint -> immediate = predecessor) ->
    dag_reaches ancestor endpoint ->
    ancestor = endpoint \/ dag_reaches ancestor predecessor.
Proof.
  intros predecessor endpoint ancestor Hunique Hreaches.
  destruct (reaches_last_edge ancestor endpoint Hreaches)
    as [Heq | [immediate [Hprefix Hedge]]].
  - left. exact Heq.
  - right.
    rewrite <- (Hunique immediate Hedge).
    exact Hprefix.
Qed.

Theorem unchanged_linear_snapshot_reuse_sound :
  forall predecessor parent candidate
    (eligible : Block -> Prop),
    (forall immediate,
      parent_edge immediate parent -> immediate = predecessor) ->
    (eligible parent -> exists validator, pairwise_support parent validator) ->
    (forall validator, ~ pairwise_support parent validator) ->
    eligible candidate ->
    dag_reaches candidate parent ->
    dag_reaches candidate predecessor.
Proof.
  intros predecessor parent candidate eligible Hunique
    Heligible_support Hparent_unsupported Hcandidate Hreaches.
  destruct (reaches_unique_predecessor
    predecessor parent candidate Hunique Hreaches) as [Heq | Hprior].
  - subst candidate.
    exfalso.
    destruct (Heligible_support Hcandidate) as [validator Hsupport].
    exact (Hparent_unsupported validator Hsupport).
  - exact Hprior.
Qed.

End CoverageOptimization.

Print Assumptions coverage_decision_transparent.
Print Assumptions unchanged_linear_snapshot_reuse_sound.
