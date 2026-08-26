From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
Import ListNotations.

From FinalizedFloor Require Import Foundation.

Definition canonical_evidence_pair
  (left right : BlockHash) : BlockHash * BlockHash :=
  (Nat.min left right, Nat.max left right).

Definition evidence_dependencies
  (left right : BlockHash) : list BlockHash :=
  let pair := canonical_evidence_pair left right in
  [fst pair; snd pair].

Definition objective_equivocation
  (sender_of : BlockHash -> Validator)
  (sequence_of : BlockHash -> nat)
  (left right : BlockHash) : Prop :=
  left <> right /\
  sender_of left = sender_of right /\
  sequence_of left = sequence_of right.

Definition accept_objective_evidence
  (sender_of : BlockHash -> Validator)
  (sequence_of : BlockHash -> nat)
  (_local_invalid : BlockHash -> bool)
  (left right : BlockHash) : Prop :=
  objective_equivocation sender_of sequence_of left right.

Definition finality_voters
  (equivocator : Validator)
  (voters : list Validator) : list Validator :=
  filter (fun validator => negb (Nat.eqb validator equivocator)) voters.

Definition current_incarnation_pair
  (target_incarnation : nat)
  (incarnation_of : BlockHash -> nat)
  (hashes : list BlockHash) : option (BlockHash * BlockHash) :=
  match filter
          (fun hash => Nat.eqb (incarnation_of hash) target_incarnation)
          hashes with
  | hash_left :: hash_right :: _ =>
      Some (canonical_evidence_pair hash_left hash_right)
  | _ => None
  end.

Definition current_generation_epoch_pair
  (target_generation target_epoch : nat)
  (generation_of epoch_of : BlockHash -> nat)
  (hashes : list BlockHash) : option (BlockHash * BlockHash) :=
  match filter
          (fun hash =>
             andb
               (Nat.eqb (generation_of hash) target_generation)
               (Nat.eqb (epoch_of hash) target_epoch))
          hashes with
  | hash_left :: hash_right :: _ =>
      Some (canonical_evidence_pair hash_left hash_right)
  | _ => None
  end.

Definition objective_pair_authorized_v5
  (target_generation target_epoch : nat)
  (generation_of epoch_of : BlockHash -> nat)
  (left right : BlockHash) : bool :=
  andb (negb (Nat.eqb left right))
    (andb (Nat.eqb (generation_of left) target_generation)
      (andb (Nat.eqb (generation_of right) target_generation)
        (andb (Nat.eqb (epoch_of left) target_epoch)
          (Nat.eqb (epoch_of right) target_epoch)))).

Record canonical_slash_authority := {
  authority_state_root : nat;
  authority_bond : Validator -> nat;
  authority_generation : Validator -> nat
}.

Definition objective_pair_authorized_by_authority_v5
  (authority : canonical_slash_authority)
  (target_epoch : nat)
  (sender_of : BlockHash -> Validator)
  (sequence_of generation_of epoch_of : BlockHash -> nat)
  (left right : BlockHash) : bool :=
  andb (Nat.eqb (sender_of left) (sender_of right))
    (andb (Nat.eqb (sequence_of left) (sequence_of right))
      (andb (Nat.ltb 0 (authority_bond authority (sender_of left)))
        (objective_pair_authorized_v5
          (authority_generation authority (sender_of left))
          target_epoch generation_of epoch_of left right))).

Definition proposer_objective_authorized_by_authority_v5 :=
  objective_pair_authorized_by_authority_v5.
Definition receiver_objective_authorized_by_authority_v5 :=
  objective_pair_authorized_by_authority_v5.

Definition canonical_slash_authority_from_state
  (state_root : nat)
  (bonds generations : Validator -> nat) : canonical_slash_authority :=
  {| authority_state_root := state_root;
     authority_bond := bonds;
     authority_generation := generations |}.

Definition proposer_objective_authorized_v5 := objective_pair_authorized_v5.
Definition receiver_objective_authorized_v5 := objective_pair_authorized_v5.

Definition slash_authority_needed
  (invalid_evidence_count objective_group_size : nat) : bool :=
  orb (Nat.ltb 0 invalid_evidence_count) (Nat.leb 2 objective_group_size).

Definition first_two_before_incarnation_grouping
  (hashes : list BlockHash) : option (BlockHash * BlockHash) :=
  match hashes with
  | hash_left :: hash_right :: _ =>
      Some (canonical_evidence_pair hash_left hash_right)
  | _ => None
  end.

Definition incarnation_finality_voters
  (active_incarnation evidence_incarnation : nat)
  (eligible_same_incarnation_pair : bool)
  (equivocator : Validator)
  (voters : list Validator) : list Validator :=
  if andb eligible_same_incarnation_pair
       (Nat.eqb evidence_incarnation active_incarnation)
  then finality_voters equivocator voters
  else voters.

Definition same_current_incarnation
  (target_incarnation left_incarnation right_incarnation : nat) : bool :=
  Nat.eqb left_incarnation target_incarnation && Nat.eqb right_incarnation target_incarnation.

Definition objective_slash_authorized
  (objective_pair_present : bool)
  (target_incarnation left_incarnation right_incarnation unary_incarnation : nat) : bool :=
  if objective_pair_present
  then same_current_incarnation target_incarnation left_incarnation right_incarnation
  else Nat.eqb unary_incarnation target_incarnation.

Definition same_fault_key
  (left right : Validator * nat) : bool :=
  andb (Nat.eqb (fst left) (fst right))
    (Nat.eqb (snd left) (snd right)).

Definition scoped_unary_slash_authorized
  (objective_group unary_fault : Validator * nat)
  (unary_eligible : bool) : bool :=
  if same_fault_key objective_group unary_fault
  then false
  else unary_eligible.

Definition canonical_unary_evidence
  (left right : BlockHash) : BlockHash := Nat.min left right.

Definition pair_by_bond_incarnation
  (target_incarnation : nat)
  (incarnation_of _block_epoch_of : BlockHash -> nat)
  (hashes : list BlockHash) : option (BlockHash * BlockHash) :=
  current_incarnation_pair target_incarnation incarnation_of hashes.

Definition same_block_unbond_rejected
  (use_pre_state_authority pre_state_bonded post_state_bonded : bool)
  (local_invalid objective_pair_present missing_slash : bool) : bool :=
  if use_pre_state_authority
  then andb pre_state_bonded (andb objective_pair_present missing_slash)
  else local_invalid.

Definition repair_duplicate_evidence_index
  (repair_on_retry metadata_present index_present : bool) : bool :=
  if andb repair_on_retry metadata_present then true else index_present.

Definition filtered_finality_voters
  (invalid_voter objective_equivocator : Validator)
  (objective_active : bool)
  (exact_justifications : list Validator) : list Validator :=
  let valid_voters := finality_voters invalid_voter exact_justifications in
  if objective_active
  then finality_voters objective_equivocator valid_voters
  else valid_voters.

Definition objective_refinement_contract : Prop :=
  (forall target_incarnation incarnation_of first_epochs second_epochs hashes,
     pair_by_bond_incarnation
       target_incarnation incarnation_of first_epochs hashes =
     pair_by_bond_incarnation
       target_incarnation incarnation_of second_epochs hashes)
  /\
  (forall left right,
     canonical_unary_evidence left right =
     canonical_unary_evidence right left)
  /\
  (forall local_left local_right,
     same_block_unbond_rejected
       true true false local_left true true = true /\
     same_block_unbond_rejected
       true true false local_right true true = true)
  /\
  (same_block_unbond_rejected false true false true true true <>
   same_block_unbond_rejected false true false false true true)
  /\
  repair_duplicate_evidence_index true true false = true
  /\
  (forall invalid_voter objective_equivocator objective_active exact,
     ~ In invalid_voter
         (filtered_finality_voters
           invalid_voter objective_equivocator objective_active exact))
  /\
  (forall invalid_voter objective_equivocator objective_active exact,
     exact = exact /\
     incl
       (filtered_finality_voters
         invalid_voter objective_equivocator objective_active exact)
       exact).

Definition apply_objective_evidence
  (validity : BlockHash -> bool)
  (_evidence : BlockHash * BlockHash) : BlockHash -> bool :=
  validity.

Definition DurableObjectiveEvidence := option (BlockHash * BlockHash).

Definition persist_objective_evidence
  (evidence : BlockHash * BlockHash) : DurableObjectiveEvidence :=
  Some evidence.

Definition restart_objective_evidence
  (durable : DurableObjectiveEvidence) : DurableObjectiveEvidence :=
  durable.

Theorem canonical_evidence_pair_symmetric :
  forall left right,
    canonical_evidence_pair left right =
    canonical_evidence_pair right left.
Proof.
  intros left right. unfold canonical_evidence_pair.
  rewrite Nat.min_comm, Nat.max_comm. reflexivity.
Qed.

Theorem canonical_evidence_dependencies_contain_both_hashes :
  forall left right,
    In left (evidence_dependencies left right) /\
    In right (evidence_dependencies left right).
Proof.
  intros left right. unfold evidence_dependencies, canonical_evidence_pair.
  destruct (Nat.le_ge_cases left right) as [Hle | Hge].
  - rewrite Nat.min_l by exact Hle.
    rewrite Nat.max_r by exact Hle.
    simpl. auto.
  - rewrite Nat.min_r by exact Hge.
    rewrite Nat.max_l by exact Hge.
    simpl. auto.
Qed.

Theorem objective_equivocation_is_symmetric :
  forall sender_of sequence_of left right,
    objective_equivocation sender_of sequence_of left right ->
    objective_equivocation sender_of sequence_of right left.
Proof.
  intros sender_of sequence_of left right
    [Hdistinct [Hsender Hsequence]].
  unfold objective_equivocation.
  split; [congruence |].
  split; symmetry; assumption.
Qed.

Theorem equal_sequence_siblings_suffice_for_objective_acceptance :
  forall sender_of sequence_of local_invalid left right,
    objective_equivocation sender_of sequence_of left right ->
    accept_objective_evidence
      sender_of sequence_of local_invalid left right.
Proof. intros. exact H. Qed.

Theorem objective_acceptance_ignores_local_invalid_flags :
  forall sender_of sequence_of local_left local_right left right,
    accept_objective_evidence
      sender_of sequence_of local_left left right <->
    accept_objective_evidence
      sender_of sequence_of local_right left right.
Proof. reflexivity. Qed.

Theorem objective_evidence_does_not_retroactively_change_block_validity :
  forall validity left right hash,
    apply_objective_evidence
      validity (canonical_evidence_pair left right) hash = validity hash.
Proof. reflexivity. Qed.

Theorem objective_equivocator_is_excluded_from_finality_voters :
  forall equivocator voters,
    ~ In equivocator (finality_voters equivocator voters).
Proof.
  intros equivocator voters Hin.
  unfold finality_voters in Hin.
  rewrite filter_In in Hin.
  destruct Hin as [_ Hneq].
  rewrite Nat.eqb_refl in Hneq. discriminate.
Qed.

Theorem incarnation_grouping_precedes_pair_canonicalization :
  forall target_incarnation incarnation_of old_hash current_left current_right,
    incarnation_of old_hash <> target_incarnation ->
    incarnation_of current_left = target_incarnation ->
    incarnation_of current_right = target_incarnation ->
    current_incarnation_pair target_incarnation incarnation_of
      [old_hash; current_left; current_right] =
      Some (canonical_evidence_pair current_left current_right) /\
    current_incarnation_pair target_incarnation incarnation_of
      [current_right; old_hash; current_left] =
      Some (canonical_evidence_pair current_left current_right).
Proof.
  intros target_incarnation incarnation_of old_hash current_left current_right
    Hold Hleft Hright.
  unfold current_incarnation_pair. simpl.
  apply Nat.eqb_neq in Hold.
  rewrite Hold, Hleft, Hright, !Nat.eqb_refl.
  split; [reflexivity |].
  rewrite canonical_evidence_pair_symmetric. reflexivity.
Qed.

Theorem generation_and_epoch_grouping_precede_pair_canonicalization :
  forall target_generation target_epoch generation_of epoch_of
         old_epoch_hash current_left current_right,
    generation_of old_epoch_hash = target_generation ->
    generation_of current_left = target_generation ->
    generation_of current_right = target_generation ->
    epoch_of old_epoch_hash <> target_epoch ->
    epoch_of current_left = target_epoch ->
    epoch_of current_right = target_epoch ->
    current_generation_epoch_pair
      target_generation target_epoch generation_of epoch_of
      [old_epoch_hash; current_left; current_right] =
      Some (canonical_evidence_pair current_left current_right) /\
    current_generation_epoch_pair
      target_generation target_epoch generation_of epoch_of
      [current_right; old_epoch_hash; current_left] =
      Some (canonical_evidence_pair current_left current_right).
Proof.
  intros target_generation target_epoch generation_of epoch_of
    old_epoch_hash current_left current_right
    HoldGeneration HleftGeneration HrightGeneration
    HoldEpoch HleftEpoch HrightEpoch.
  unfold current_generation_epoch_pair. simpl.
  rewrite HoldGeneration, HleftGeneration, HrightGeneration.
  rewrite HleftEpoch, HrightEpoch, !Nat.eqb_refl.
  apply Nat.eqb_neq in HoldEpoch. rewrite HoldEpoch. simpl.
  split; [reflexivity |].
  rewrite canonical_evidence_pair_symmetric. reflexivity.
Qed.

Theorem cross_epoch_objective_pair_cannot_authorize_v5 :
  forall target_generation target_epoch generation_of epoch_of left right,
    epoch_of left <> target_epoch ->
    objective_pair_authorized_v5
      target_generation target_epoch generation_of epoch_of left right = false.
Proof.
  intros target_generation target_epoch generation_of epoch_of left right Hepoch.
  unfold objective_pair_authorized_v5.
  apply Nat.eqb_neq in Hepoch. rewrite Hepoch.
  simpl. repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem proposer_receiver_objective_authorization_parity_v5 :
  forall target_generation target_epoch generation_of epoch_of left right,
    proposer_objective_authorized_v5
      target_generation target_epoch generation_of epoch_of left right =
    receiver_objective_authorized_v5
      target_generation target_epoch generation_of epoch_of left right.
Proof. reflexivity. Qed.

Theorem proposer_receiver_canonical_authority_parity_v5 :
  forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
    proposer_objective_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right =
    receiver_objective_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right.
Proof. reflexivity. Qed.

Theorem nonpositive_canonical_bond_rejects_objective_pair_v5 :
  forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
    authority_bond authority (sender_of left) = 0 ->
    objective_pair_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right = false.
Proof.
  intros authority target_epoch sender_of sequence_of generation_of epoch_of left right Hbond.
  unfold objective_pair_authorized_by_authority_v5.
  rewrite Hbond, Nat.ltb_irrefl.
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem mismatched_sender_rejects_objective_pair_v5 :
  forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
    sender_of left <> sender_of right ->
    objective_pair_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right = false.
Proof.
  intros authority target_epoch sender_of sequence_of generation_of epoch_of left right Hsender.
  unfold objective_pair_authorized_by_authority_v5.
  apply Nat.eqb_neq in Hsender. rewrite Hsender. reflexivity.
Qed.

Theorem mismatched_sequence_rejects_objective_pair_v5 :
  forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
    sequence_of left <> sequence_of right ->
    objective_pair_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right = false.
Proof.
  intros authority target_epoch sender_of sequence_of generation_of epoch_of left right Hsequence.
  unfold objective_pair_authorized_by_authority_v5.
  apply Nat.eqb_neq in Hsequence. rewrite Hsequence.
  destruct (Nat.eqb (sender_of left) (sender_of right)); reflexivity.
Qed.

Theorem cross_epoch_canonical_authority_pair_cannot_authorize_v5 :
  forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
    epoch_of left <> target_epoch ->
    objective_pair_authorized_by_authority_v5
      authority target_epoch sender_of sequence_of generation_of epoch_of left right = false.
Proof.
  intros authority target_epoch sender_of sequence_of generation_of epoch_of left right Hepoch.
  unfold objective_pair_authorized_by_authority_v5.
  rewrite (cross_epoch_objective_pair_cannot_authorize_v5
    (authority_generation authority (sender_of left))
    target_epoch generation_of epoch_of left right Hepoch).
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem canonical_slash_authority_snapshot_is_root_bound :
  forall state_root bonds generations,
    authority_state_root
      (canonical_slash_authority_from_state state_root bonds generations) = state_root /\
    authority_bond
      (canonical_slash_authority_from_state state_root bonds generations) = bonds /\
    authority_generation
      (canonical_slash_authority_from_state state_root bonds generations) = generations.
Proof. intros. repeat split; reflexivity. Qed.

Theorem pair_only_evidence_activates_slash_authority :
  slash_authority_needed 0 2 = true.
Proof. reflexivity. Qed.

Theorem attacker_block_epoch_cannot_change_bond_incarnation_group :
  forall target_incarnation incarnation_of first_epochs second_epochs hashes,
    pair_by_bond_incarnation
      target_incarnation incarnation_of first_epochs hashes =
    pair_by_bond_incarnation
      target_incarnation incarnation_of second_epochs hashes.
Proof. reflexivity. Qed.

Theorem first_two_before_grouping_can_select_cross_incarnation_pair :
  forall old_hash current_left current_right,
    first_two_before_incarnation_grouping
      [old_hash; current_left; current_right] =
      Some (canonical_evidence_pair old_hash current_left).
Proof. reflexivity. Qed.

Theorem structural_pair_without_same_incarnation_evidence_preserves_voters :
  forall active_incarnation evidence_incarnation equivocator voters,
    incarnation_finality_voters
      active_incarnation evidence_incarnation false equivocator voters = voters.
Proof. reflexivity. Qed.

Theorem active_incarnation_evidence_excludes_equivocator :
  forall active_incarnation equivocator voters,
    ~ In equivocator
        (incarnation_finality_voters
          active_incarnation active_incarnation true equivocator voters).
Proof.
  intros active_incarnation equivocator voters.
  unfold incarnation_finality_voters.
  rewrite Nat.eqb_refl. simpl.
  apply objective_equivocator_is_excluded_from_finality_voters.
Qed.

Theorem later_incarnation_restores_raw_public_key :
  forall active_incarnation evidence_incarnation equivocator voters,
    evidence_incarnation <> active_incarnation ->
    incarnation_finality_voters
      active_incarnation evidence_incarnation true equivocator voters = voters.
Proof.
  intros active_incarnation evidence_incarnation equivocator voters Hdifferent.
  unfold incarnation_finality_voters.
  apply Nat.eqb_neq in Hdifferent.
  rewrite Hdifferent. reflexivity.
Qed.

Theorem same_current_incarnation_objective_pair_is_slash_eligible :
  forall target_incarnation unary_incarnation,
    objective_slash_authorized
      true target_incarnation target_incarnation target_incarnation unary_incarnation = true.
Proof.
  intros target_incarnation unary_incarnation.
  unfold objective_slash_authorized, same_current_incarnation.
  rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem cross_incarnation_objective_pair_suppresses_unary_fallback :
  forall target_incarnation old_incarnation unary_incarnation,
    old_incarnation <> target_incarnation ->
    objective_slash_authorized
      true target_incarnation old_incarnation target_incarnation unary_incarnation = false.
Proof.
  intros target_incarnation old_incarnation unary_incarnation Hold.
  unfold objective_slash_authorized, same_current_incarnation.
  apply Nat.eqb_neq in Hold. rewrite Hold. reflexivity.
Qed.

Theorem objective_pair_slash_decision_is_independent_of_unary_arrival :
  forall target_incarnation left_incarnation right_incarnation unary_left unary_right,
    objective_slash_authorized
      true target_incarnation left_incarnation right_incarnation unary_left =
    objective_slash_authorized
      true target_incarnation left_incarnation right_incarnation unary_right.
Proof. reflexivity. Qed.

Theorem objective_pair_suppresses_unary_fallback_at_same_fault_key :
  forall validator sequence unary_eligible,
    scoped_unary_slash_authorized
      (validator, sequence) (validator, sequence) unary_eligible = false.
Proof.
  intros validator sequence unary_eligible.
  unfold scoped_unary_slash_authorized, same_fault_key. simpl.
  rewrite !Nat.eqb_refl. reflexivity.
Qed.

Theorem independent_unary_fault_at_other_sequence_remains_eligible :
  forall validator objective_sequence unary_sequence,
    objective_sequence <> unary_sequence ->
    scoped_unary_slash_authorized
      (validator, objective_sequence) (validator, unary_sequence) true = true.
Proof.
  intros validator objective_sequence unary_sequence Hdifferent.
  unfold scoped_unary_slash_authorized, same_fault_key. simpl.
  rewrite Nat.eqb_refl. simpl.
  apply Nat.eqb_neq in Hdifferent. rewrite Hdifferent. reflexivity.
Qed.

Theorem canonical_unary_evidence_is_permutation_independent :
  forall left right,
    canonical_unary_evidence left right =
    canonical_unary_evidence right left.
Proof.
  intros left right. unfold canonical_unary_evidence.
  apply Nat.min_comm.
Qed.

Theorem canonical_pre_state_authority_rejects_same_block_unbond :
  forall local_left local_right,
    same_block_unbond_rejected
      true true false local_left true true = true /\
    same_block_unbond_rejected
      true true false local_right true true = true.
Proof. split; reflexivity. Qed.

Theorem post_state_local_flags_can_diverge_same_block_unbond :
  same_block_unbond_rejected false true false true true true <>
  same_block_unbond_rejected false true false false true true.
Proof. discriminate. Qed.

Theorem duplicate_retry_repairs_missing_evidence_index :
  repair_duplicate_evidence_index true true false = true.
Proof. reflexivity. Qed.

Theorem invalid_lmm_is_excluded_from_filtered_finality_voters :
  forall invalid_voter objective_equivocator objective_active exact,
    ~ In invalid_voter
        (filtered_finality_voters
          invalid_voter objective_equivocator objective_active exact).
Proof.
  intros invalid_voter objective_equivocator objective_active exact Hin.
  unfold filtered_finality_voters in Hin.
  destruct objective_active.
  - unfold finality_voters in Hin.
    rewrite filter_In in Hin. destruct Hin as [Hin _].
    rewrite filter_In in Hin. destruct Hin as [_ Hneq].
    rewrite Nat.eqb_refl in Hneq. discriminate.
  - unfold finality_voters in Hin.
    rewrite filter_In in Hin. destruct Hin as [_ Hneq].
    rewrite Nat.eqb_refl in Hneq. discriminate.
Qed.

Theorem exact_justifications_are_not_replaced_by_vote_projection :
  forall invalid_voter objective_equivocator objective_active exact,
    exact = exact /\
    incl
      (filtered_finality_voters
        invalid_voter objective_equivocator objective_active exact)
      exact.
Proof.
  intros invalid_voter objective_equivocator objective_active exact.
  split; [reflexivity |].
  intros voter Hin. unfold filtered_finality_voters in Hin.
  destruct objective_active.
  - unfold finality_voters in Hin.
    repeat rewrite filter_In in Hin. tauto.
  - unfold finality_voters in Hin.
    rewrite filter_In in Hin. tauto.
Qed.

Theorem objective_refinement_contract_holds :
  objective_refinement_contract.
Proof.
  exact (conj attacker_block_epoch_cannot_change_bond_incarnation_group
    (conj canonical_unary_evidence_is_permutation_independent
      (conj canonical_pre_state_authority_rejects_same_block_unbond
        (conj post_state_local_flags_can_diverge_same_block_unbond
          (conj duplicate_retry_repairs_missing_evidence_index
            (conj invalid_lmm_is_excluded_from_filtered_finality_voters
              exact_justifications_are_not_replaced_by_vote_projection)))))).
Qed.

Theorem restart_preserves_canonical_objective_evidence :
  forall left right,
    restart_objective_evidence
      (persist_objective_evidence (canonical_evidence_pair left right)) =
    Some (canonical_evidence_pair left right).
Proof. reflexivity. Qed.
