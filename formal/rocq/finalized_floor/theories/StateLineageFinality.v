From Stdlib Require Import Lists.List.
From Stdlib Require Import Relations.Relation_Operators.
Import ListNotations.

Section General.

Context {Block : Type}.
Variable certified : Block -> Prop.
Variable state_ancestor : Block -> Block -> Prop.

Definition lfb_eligible (current candidate : Block) : Prop :=
  certified candidate /\
  state_ancestor current candidate.

Record finality_state : Type := {
  current_lfb : Block;
  committed_blocks : list Block
}.

Definition lineage_invariant (state : finality_state) : Prop :=
  Forall
    (fun committed => state_ancestor committed (current_lfb state))
    (committed_blocks state).

Definition promote (state : finality_state) (candidate : Block) : finality_state :=
  {| current_lfb := candidate;
     committed_blocks := candidate :: committed_blocks state |}.

Theorem eligibility_preserves_certificate :
  forall current candidate,
    lfb_eligible current candidate ->
    certified candidate.
Proof.
  intros current candidate [Hcertified _].
  exact Hcertified.
Qed.

Theorem certified_stale_candidate_is_ineligible :
  forall current candidate,
    certified candidate ->
    ~ state_ancestor current candidate ->
    ~ lfb_eligible current candidate.
Proof.
  intros current candidate Hcertified Hstate
    [_ Heligible_state].
  exact (Hstate Heligible_state).
Qed.

Theorem certified_rebase_is_eligible :
  forall current candidate,
    certified candidate ->
    state_ancestor current candidate ->
    lfb_eligible current candidate.
Proof.
  intros current candidate Hcertified Hstate.
  exact (conj Hcertified Hstate).
Qed.

Theorem certified_off_main_rebase_is_eligible :
  forall
    (main_ancestor : Block -> Block -> Prop)
    current candidate,
    certified candidate ->
    ~ main_ancestor current candidate ->
    state_ancestor current candidate ->
    lfb_eligible current candidate.
Proof.
  intros main_ancestor current candidate Hcertified _ Hstate.
  exact (conj Hcertified Hstate).
Qed.

Theorem eligible_promotion_preserves_lineage :
  (forall block, state_ancestor block block) ->
  (forall left middle right,
    state_ancestor left middle ->
    state_ancestor middle right ->
    state_ancestor left right) ->
  forall state candidate,
    lineage_invariant state ->
    lfb_eligible (current_lfb state) candidate ->
    lineage_invariant (promote state candidate).
Proof.
  intros Hreflexive Htransitive state candidate Hinvariant
    [_ Hcurrent_candidate].
  unfold lineage_invariant, promote.
  simpl.
  constructor.
  - apply Hreflexive.
  - induction Hinvariant as [|committed remaining Hcommitted Hremaining IH].
    + constructor.
    + constructor.
      * eapply Htransitive.
        -- exact Hcommitted.
        -- exact Hcurrent_candidate.
      * exact IH.
Qed.

End General.

Section StateBase.

Context {Block : Type}.

Definition state_edge (base : Block -> Block) (ancestor descendant : Block) : Prop :=
  base descendant = ancestor.

Definition base_state_ancestor (base : Block -> Block) : Block -> Block -> Prop :=
  clos_refl_trans Block (state_edge base).

Theorem base_state_ancestor_reflexive :
  forall base block,
    base_state_ancestor base block block.
Proof.
  intros base block.
  apply rt_refl.
Qed.

Theorem base_state_ancestor_transitive :
  forall base left middle right,
    base_state_ancestor base left middle ->
    base_state_ancestor base middle right ->
    base_state_ancestor base left right.
Proof.
  intros base left middle right Hleft Hright.
  eapply rt_trans.
  - exact Hleft.
  - exact Hright.
Qed.

End StateBase.

Inductive scenario_block : Type :=
| Genesis
| Funding
| Sibling
| Stale
| Rebased.

Definition scenario_certified (block : scenario_block) : Prop :=
  match block with
  | Sibling => False
  | _ => True
  end.

Definition scenario_main_ancestor
  (ancestor descendant : scenario_block) : Prop :=
  match ancestor, descendant with
  | Genesis, _ => True
  | Funding, Funding => True
  | Funding, Stale => True
  | Sibling, Sibling => True
  | Sibling, Rebased => True
  | Stale, Stale => True
  | Stale, Rebased => True
  | Rebased, Rebased => True
  | _, _ => False
  end.

Definition scenario_state_ancestor
  (ancestor descendant : scenario_block) : Prop :=
  match ancestor, descendant with
  | Genesis, _ => True
  | Funding, Funding => True
  | Funding, Rebased => True
  | Sibling, Sibling => True
  | Stale, Stale => True
  | Rebased, Rebased => True
  | _, _ => False
  end.

Definition scenario_initial_state : @finality_state scenario_block :=
  {| current_lfb := Funding;
     committed_blocks := [Funding; Genesis] |}.

Definition state_lineage_contract : Prop :=
  scenario_certified Stale /\
  scenario_main_ancestor Funding Stale /\
  ~ scenario_state_ancestor Funding Stale /\
  ~ lfb_eligible
      scenario_certified
      scenario_state_ancestor
      Funding
      Stale /\
  ~ scenario_main_ancestor Funding Rebased /\
  scenario_main_ancestor Sibling Rebased /\
  lfb_eligible
      scenario_certified
      scenario_state_ancestor
      Funding
      Rebased /\
  lineage_invariant
      scenario_state_ancestor
      scenario_initial_state /\
  ~ lineage_invariant
      scenario_state_ancestor
      (promote scenario_initial_state Stale) /\
  lineage_invariant
      scenario_state_ancestor
      (promote scenario_initial_state Rebased).

Theorem state_lineage_end_to_end : state_lineage_contract.
Proof.
  unfold state_lineage_contract, scenario_initial_state,
    scenario_certified, scenario_main_ancestor,
    scenario_state_ancestor, lfb_eligible,
    lineage_invariant, promote.
  repeat split; simpl; try tauto.
  - constructor.
    + exact I.
    + constructor.
      * exact I.
      * constructor.
  - intros Hlineage.
    inversion Hlineage as [|stale tail Hstale Htail]; subst.
    inversion Htail as [|funding remaining Hfunding Hremaining]; subst.
    exact Hfunding.
  - constructor.
    + exact I.
    + constructor.
      * exact I.
      * constructor.
        -- exact I.
        -- constructor.
Qed.

Definition promotion_preservation_contract : Prop :=
  forall
    (Block : Type)
    (certified : Block -> Prop)
    (state_ancestor : Block -> Block -> Prop),
    (forall block, state_ancestor block block) ->
    (forall left middle right,
      state_ancestor left middle ->
      state_ancestor middle right ->
      state_ancestor left right) ->
    forall state candidate,
      lineage_invariant state_ancestor state ->
      lfb_eligible certified state_ancestor
        (current_lfb state) candidate ->
      lineage_invariant state_ancestor (promote state candidate).

Theorem state_lineage_promotion_correct : promotion_preservation_contract.
Proof.
  unfold promotion_preservation_contract.
  intros Block certified state_ancestor
    Hreflexive Htransitive state candidate Hinvariant Heligible.
  eapply eligible_promotion_preserves_lineage.
  - exact Hreflexive.
  - exact Htransitive.
  - exact Hinvariant.
  - exact Heligible.
Qed.

Definition base_lineage_promotion_contract : Prop :=
  forall
    (Block : Type)
    (base : Block -> Block)
    (certified : Block -> Prop)
    (state : @finality_state Block)
    (candidate : Block),
    lineage_invariant (base_state_ancestor base) state ->
    lfb_eligible certified (base_state_ancestor base)
      (current_lfb state) candidate ->
    lineage_invariant (base_state_ancestor base) (promote state candidate).

Theorem base_lineage_promotion_correct : base_lineage_promotion_contract.
Proof.
  unfold base_lineage_promotion_contract.
  intros Block base certified state candidate Hinvariant Heligible.
  eapply eligible_promotion_preserves_lineage.
  - apply base_state_ancestor_reflexive.
  - apply base_state_ancestor_transitive.
  - exact Hinvariant.
  - exact Heligible.
Qed.
