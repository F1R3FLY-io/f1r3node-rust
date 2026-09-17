From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From FinalizedFloor Require Import OccurrenceDisposition.

Record effect_receipt := {
  receipt_occurrence : occurrence;
  receipt_chain : nat;
  receipt_execution : nat;
  receipt_ordinary : nat;
  receipt_merge_metadata : nat
}.

Definition receipt_deploy (receipt : effect_receipt) : nat :=
  deploy_id (receipt_occurrence receipt).

Definition receipt_source (receipt : effect_receipt) : nat :=
  source_id (receipt_occurrence receipt).

Definition receipt_set := effect_receipt -> Prop.
Definition tombstone_set := occurrence -> Prop.

Definition same_deploy (left right : effect_receipt) : Prop :=
  receipt_deploy left = receipt_deploy right.

Definition base_spent
  (base : receipt_set)
  (candidate : effect_receipt) : Prop :=
  exists committed, base committed /\ same_deploy committed candidate.

Definition base_chain_spent
  (base scope : receipt_set)
  (candidate : effect_receipt) : Prop :=
  exists named,
    scope named /\
    base_spent base named /\
    receipt_chain named = receipt_chain candidate.

Definition chain_tombstoned
  (scope : receipt_set)
  (tombstones : tombstone_set)
  (candidate : effect_receipt) : Prop :=
  exists named,
    scope named /\
    tombstones (receipt_occurrence named) /\
    receipt_chain named = receipt_chain candidate.

Definition eligible
  (base scope : receipt_set)
  (tombstones : tombstone_set)
  (candidate : effect_receipt) : Prop :=
  scope candidate /\
  ~ chain_tombstoned scope tombstones candidate /\
  ~ base_chain_spent base scope candidate.

Definition key_le (left right : effect_receipt) : Prop :=
  receipt_source left < receipt_source right \/
  (receipt_source left = receipt_source right /\
   receipt_execution left <= receipt_execution right).

Definition selected
  (base scope : receipt_set)
  (tombstones : tombstone_set)
  (candidate : effect_receipt) : Prop :=
  eligible base scope tombstones candidate /\
  forall other,
    eligible base scope tombstones other ->
    same_deploy candidate other ->
    key_le candidate other.

Definition committed
  (base scope : receipt_set)
  (tombstones : tombstone_set)
  (receipt : effect_receipt) : Prop :=
  base receipt \/ selected base scope tombstones receipt.

Definition base_deploy_unique (base : receipt_set) : Prop :=
  forall left right,
    base left ->
    base right ->
    same_deploy left right ->
    left = right.

Definition effect_identity_consistent (receipts : receipt_set) : Prop :=
  forall left right,
    receipts left ->
    receipts right ->
    receipt_source left = receipt_source right ->
    receipt_execution left = receipt_execution right ->
    left = right.

Definition ordinary_applied
  (base scope : receipt_set)
  (tombstones : tombstone_set) : receipt_set :=
  committed base scope tombstones.

Definition merge_metadata_bound
  (base scope : receipt_set)
  (tombstones : tombstone_set) : receipt_set :=
  committed base scope tombstones.

Lemma key_le_antisymmetric_identity :
  forall left right,
    key_le left right ->
    key_le right left ->
    receipt_source left = receipt_source right /\
    receipt_execution left = receipt_execution right.
Proof.
  intros left right Hleft Hright.
  unfold key_le in Hleft, Hright.
  destruct Hleft as [HsourceLeft | [HsourceEq HexecLeft]];
  destruct Hright as [HsourceRight | [HsourceEq' HexecRight]]; lia.
Qed.

Theorem base_receipt_is_committed :
  forall base scope tombstones receipt,
    base receipt -> committed base scope tombstones receipt.
Proof.
  intros base scope tombstones receipt Hbase.
  left. exact Hbase.
Qed.

Theorem base_committed_dominates_scope :
  forall base scope tombstones committed_receipt candidate,
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ selected base scope tombstones candidate.
Proof.
  intros base scope tombstones committed_receipt candidate Hbase Hsame Hselected.
  destruct Hselected as [[Hscope [_ HnotSpent]] _].
  apply HnotSpent.
  exists candidate. repeat split.
  - exact Hscope.
  - exists committed_receipt. split; assumption.
Qed.

Theorem tombstone_is_scope_local :
  forall scope tombstones candidate,
    chain_tombstoned scope tombstones candidate ->
    exists named,
      scope named /\
      tombstones (receipt_occurrence named) /\
      receipt_chain named = receipt_chain candidate.
Proof.
  intros scope tombstones candidate H.
  exact H.
Qed.

Theorem tombstoned_chain_is_excluded :
  forall base scope tombstones named candidate,
    scope named ->
    tombstones (receipt_occurrence named) ->
    receipt_chain named = receipt_chain candidate ->
    ~ selected base scope tombstones candidate.
Proof.
  intros base scope tombstones named candidate Hscope Htombstone Hchain Hselected.
  destruct Hselected as [[_ [HnotTombstoned _]] _].
  apply HnotTombstoned.
  exists named. repeat split; assumption.
Qed.

Theorem base_duplicate_chain_is_excluded :
  forall base scope tombstones named candidate committed_receipt,
    scope named ->
    base committed_receipt ->
    same_deploy committed_receipt named ->
    receipt_chain named = receipt_chain candidate ->
    ~ selected base scope tombstones candidate.
Proof.
  intros base scope tombstones named candidate committed_receipt.
  intros Hscope Hbase Hdeploy Hchain Hselected.
  destruct Hselected as [[_ [_ HnotSpent]] _].
  apply HnotSpent.
  exists named. repeat split.
  - exact Hscope.
  - exists committed_receipt. split; assumption.
  - exact Hchain.
Qed.

Theorem selected_is_scope_member :
  forall base scope tombstones receipt,
    selected base scope tombstones receipt -> scope receipt.
Proof.
  intros base scope tombstones receipt Hselected.
  exact (proj1 (proj1 Hselected)).
Qed.

Theorem selected_deploy_unique :
  forall base scope tombstones,
    effect_identity_consistent scope ->
    forall left right,
      selected base scope tombstones left ->
      selected base scope tombstones right ->
      same_deploy left right ->
      left = right.
Proof.
  intros base scope tombstones Hidentity left right Hleft Hright Hdeploy.
  destruct Hleft as [HleftEligible HleftMin].
  destruct Hright as [HrightEligible HrightMin].
  pose proof (HleftMin right HrightEligible Hdeploy) as HleftKey.
  pose proof (HrightMin left HleftEligible (eq_sym Hdeploy)) as HrightKey.
  destruct (key_le_antisymmetric_identity left right HleftKey HrightKey)
    as [Hsource Hexecution].
  apply Hidentity.
  - exact (proj1 HleftEligible).
  - exact (proj1 HrightEligible).
  - exact Hsource.
  - exact Hexecution.
Qed.

Lemma committed_is_candidate :
  forall base scope tombstones receipt,
    committed base scope tombstones receipt ->
    base receipt \/ scope receipt.
Proof.
  intros base scope tombstones receipt Hcommitted.
  destruct Hcommitted as [Hbase | Hselected].
  - left. exact Hbase.
  - right. exact (selected_is_scope_member base scope tombstones receipt Hselected).
Qed.

Theorem committed_deploy_unique :
  forall base scope tombstones,
    base_deploy_unique base ->
    effect_identity_consistent scope ->
    forall left right,
      committed base scope tombstones left ->
      committed base scope tombstones right ->
      same_deploy left right ->
      left = right.
Proof.
  intros base scope tombstones HbaseUnique HscopeIdentity.
  intros left right Hleft Hright Hdeploy.
  destruct Hleft as [HleftBase | HleftSelected];
  destruct Hright as [HrightBase | HrightSelected].
  - apply HbaseUnique; assumption.
  - exfalso.
    eapply (base_committed_dominates_scope
      base scope tombstones left right HleftBase Hdeploy).
    exact HrightSelected.
  - exfalso.
    eapply (base_committed_dominates_scope
      base scope tombstones right left HrightBase (eq_sym Hdeploy)).
    exact HleftSelected.
  - eapply selected_deploy_unique; eauto.
Qed.

Theorem state_record_effect_coherence :
  forall base scope tombstones receipt,
    committed base scope tombstones receipt <->
    ordinary_applied base scope tombstones receipt /\
    merge_metadata_bound base scope tombstones receipt.
Proof.
  intros base scope tombstones receipt.
  unfold ordinary_applied, merge_metadata_bound.
  tauto.
Qed.

Theorem committed_effect_identity_consistent :
  forall base scope tombstones,
    effect_identity_consistent
      (fun receipt => base receipt \/ scope receipt) ->
    effect_identity_consistent
      (committed base scope tombstones).
Proof.
  intros base scope tombstones Hidentity left right Hleft Hright.
  apply Hidentity.
  - apply committed_is_candidate with tombstones. exact Hleft.
  - apply committed_is_candidate with tombstones. exact Hright.
Qed.

Definition retry_allowed
  (base scope : receipt_set)
  (tombstones : tombstone_set)
  (deploy : nat) : Prop :=
  ~ exists receipt,
      committed base scope tombstones receipt /\
      receipt_deploy receipt = deploy.

Theorem base_committed_blocks_retry :
  forall base scope tombstones receipt,
    base receipt ->
    ~ retry_allowed base scope tombstones (receipt_deploy receipt).
Proof.
  intros base scope tombstones receipt Hbase Hretry.
  apply Hretry.
  exists receipt. split.
  - apply base_receipt_is_committed. exact Hbase.
  - reflexivity.
Qed.

Theorem uncommitted_scope_candidate_can_be_selected :
  forall base scope tombstones candidate,
    eligible base scope tombstones candidate ->
    (forall other,
      eligible base scope tombstones other ->
      same_deploy candidate other ->
      key_le candidate other) ->
    selected base scope tombstones candidate.
Proof.
  intros base scope tombstones candidate Heligible Hminimum.
  split; assumption.
Qed.

Definition number_total (values : list nat) : nat :=
  fold_right Nat.add 0 values.

Definition materialize_number (base : nat) (contributions : list nat) : list nat :=
  [base + number_total contributions].

Theorem materialized_number_is_singleton :
  forall base contributions,
    length (materialize_number base contributions) = 1.
Proof.
  intros base contributions. reflexivity.
Qed.

Theorem number_total_permutation :
  forall left right,
    Permutation left right ->
    number_total left = number_total right.
Proof.
  intros left right Hperm.
  induction Hperm.
  - reflexivity.
  - simpl. now rewrite IHHperm.
  - simpl. lia.
  - now rewrite IHHperm1, IHHperm2.
Qed.

Theorem materialized_number_permutation :
  forall base left right,
    Permutation left right ->
    materialize_number base left = materialize_number base right.
Proof.
  intros base left right Hperm.
  unfold materialize_number.
  now rewrite (number_total_permutation left right Hperm).
Qed.

Print Assumptions base_committed_dominates_scope.
Print Assumptions tombstoned_chain_is_excluded.
Print Assumptions base_duplicate_chain_is_excluded.
Print Assumptions committed_deploy_unique.
Print Assumptions state_record_effect_coherence.
Print Assumptions base_committed_blocks_retry.
Print Assumptions materialized_number_permutation.
