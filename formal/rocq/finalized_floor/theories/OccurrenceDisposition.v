From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Record occurrence := {
  deploy_id : nat;
  source_id : nat
}.

Definition occurrence_eq_dec : forall x y : occurrence, {x = y} + {x <> y}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition tombstoned (records : list occurrence) (candidate : occurrence) : Prop :=
  In candidate records.

Definition active (records : list occurrence) (candidate : occurrence) : Prop :=
  ~ tombstoned records candidate.

Definition reject_occurrence
  (records : list occurrence)
  (candidate : occurrence) : list occurrence :=
  if in_dec occurrence_eq_dec candidate records
  then records
  else candidate :: records.

Lemma reject_occurrence_membership :
  forall records rejected candidate,
    tombstoned (reject_occurrence records rejected) candidate <->
    candidate = rejected \/ tombstoned records candidate.
Proof.
  intros records rejected candidate.
  unfold tombstoned, reject_occurrence.
  destruct (in_dec occurrence_eq_dec rejected records) as [Hin | Hnotin].
  - split.
    + intro Hcandidate. right. exact Hcandidate.
    + intros [Heq | Hcandidate].
      * subst candidate. exact Hin.
      * exact Hcandidate.
  - simpl. split.
    + intros [Heq | Hcandidate].
      * left. symmetry. exact Heq.
      * right. exact Hcandidate.
    + intros [Heq | Hcandidate].
      * left. symmetry. exact Heq.
      * right. exact Hcandidate.
Qed.

Theorem rejection_is_source_exact :
  forall records rejected,
    tombstoned (reject_occurrence records rejected) rejected.
Proof.
  intros records rejected.
  apply reject_occurrence_membership.
  left. reflexivity.
Qed.

Theorem distinct_source_survives_rejection :
  forall records rejected survivor,
    deploy_id rejected = deploy_id survivor ->
    source_id rejected <> source_id survivor ->
    active records survivor ->
    active (reject_occurrence records rejected) survivor.
Proof.
  intros records rejected survivor _ Hsource Hactive Htombstoned.
  apply reject_occurrence_membership in Htombstoned.
  destruct Htombstoned as [Heq | Hold].
  - apply Hsource. now rewrite Heq.
  - exact (Hactive Hold).
Qed.

Theorem rejection_order_independent :
  forall records left right candidate,
    tombstoned (reject_occurrence (reject_occurrence records left) right) candidate <->
    tombstoned (reject_occurrence (reject_occurrence records right) left) candidate.
Proof.
  intros records left right candidate.
  repeat rewrite reject_occurrence_membership.
  tauto.
Qed.

Theorem one_winner_preserved :
  forall winner loser,
    deploy_id winner = deploy_id loser ->
    source_id winner <> source_id loser ->
    active (reject_occurrence [] loser) winner.
Proof.
  intros winner loser Hdeploy Hsource.
  apply distinct_source_survives_rejection.
  - symmetry. exact Hdeploy.
  - intro Heq. apply Hsource. symmetry. exact Heq.
  - unfold active, tombstoned. simpl. tauto.
Qed.

Definition all_sources_tombstoned
  (records occurrences : list occurrence) : Prop :=
  forall candidate, In candidate occurrences -> tombstoned records candidate.

Definition retry_eligible
  (records occurrences : list occurrence)
  (valid_after next_block lifespan : nat) : Prop :=
  all_sources_tombstoned records occurrences /\
  valid_after < next_block /\
  next_block < valid_after + lifespan.

Theorem no_active_iff_all_sources_tombstoned :
  forall records occurrences,
    (forall candidate, In candidate occurrences -> ~ active records candidate) <->
    all_sources_tombstoned records occurrences.
Proof.
  intros records occurrences. split.
  - intros H candidate Hin.
    specialize (H candidate Hin).
    unfold active, tombstoned in H.
    destruct (in_dec occurrence_eq_dec candidate records) as [Hmember | Hmissing].
    + exact Hmember.
    + exfalso. apply H. exact Hmissing.
  - intros H candidate Hin Hactive.
    apply Hactive. apply H. exact Hin.
Qed.

Theorem retry_requires_no_active_source :
  forall records occurrences valid_after next_block lifespan,
    retry_eligible records occurrences valid_after next_block lifespan ->
    forall candidate, In candidate occurrences -> ~ active records candidate.
Proof.
  intros records occurrences valid_after next_block lifespan Hretry.
  apply no_active_iff_all_sources_tombstoned.
  exact (proj1 Hretry).
Qed.

Theorem active_source_blocks_retry :
  forall records occurrences candidate valid_after next_block lifespan,
    In candidate occurrences ->
    active records candidate ->
    ~ retry_eligible records occurrences valid_after next_block lifespan.
Proof.
  intros records occurrences candidate valid_after next_block lifespan Hin Hactive Hretry.
  pose proof (retry_requires_no_active_source
    records occurrences valid_after next_block lifespan Hretry candidate Hin) as Hnone.
  exact (Hnone Hactive).
Qed.

Theorem expiry_closes_recovery :
  forall records occurrences valid_after next_block lifespan,
    valid_after + lifespan <= next_block ->
    ~ retry_eligible records occurrences valid_after next_block lifespan.
Proof.
  intros records occurrences valid_after next_block lifespan Hexpired Hretry.
  unfold retry_eligible in Hretry.
  lia.
Qed.

Definition inclusion_leader (validator_count finalized_height : nat) : nat :=
  S (finalized_height mod validator_count).

Definition inclusion_authorized
  (validator_count finalized_height proposer : nat) : Prop :=
  validator_count > 0 /\
  proposer = inclusion_leader validator_count finalized_height.

Theorem inclusion_leader_in_validator_set :
  forall validator_count finalized_height,
    validator_count > 0 ->
    1 <= inclusion_leader validator_count finalized_height <= validator_count.
Proof.
  intros validator_count finalized_height Hpositive.
  unfold inclusion_leader.
  pose proof (Nat.mod_upper_bound finalized_height validator_count) as Hbound.
  lia.
Qed.

Theorem inclusion_authorization_unique_per_finalized_view :
  forall validator_count finalized_height proposer_a proposer_b,
    inclusion_authorized validator_count finalized_height proposer_a ->
    inclusion_authorized validator_count finalized_height proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros validator_count finalized_height proposer_a proposer_b Ha Hb.
  unfold inclusion_authorized in Ha, Hb.
  destruct Ha as [_ Ha].
  destruct Hb as [_ Hb].
  now rewrite Ha, Hb.
Qed.

Definition recovery_custody_authorized
  (carrier_owner proposer : nat) : Prop :=
  proposer = carrier_owner.

Theorem recovery_custody_authorization_unique_per_carrier :
  forall carrier_owner proposer_a proposer_b,
    recovery_custody_authorized carrier_owner proposer_a ->
    recovery_custody_authorized carrier_owner proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros carrier_owner proposer_a proposer_b Ha Hb.
  unfold recovery_custody_authorized in Ha, Hb.
  now rewrite Ha, Hb.
Qed.

Theorem distinct_carrier_owners_recover_independently :
  forall owner_a owner_b,
    owner_a <> owner_b ->
    recovery_custody_authorized owner_a owner_a /\
    recovery_custody_authorized owner_b owner_b /\
    owner_a <> owner_b.
Proof.
  intros owner_a owner_b Hdistinct.
  repeat split; try reflexivity; exact Hdistinct.
Qed.
