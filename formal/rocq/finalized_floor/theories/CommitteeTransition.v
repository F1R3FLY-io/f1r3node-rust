From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.PeanoNat.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.

Definition committee_validators (committee : Committee) : list Validator :=
  map fst committee.

Definition positive_committee_validators (committee : Committee) : list Validator :=
  map fst (filter (fun bond => Nat.ltb 0 (snd bond)) committee).

Definition active_weight_committee
  (weights : Committee) (active : list Validator) : Committee :=
  filter
    (fun bond =>
       if in_dec Nat.eq_dec (fst bond) active
       then Nat.ltb 0 (snd bond)
       else false)
    weights.

Definition justification_validators (block : Block) : list Validator :=
  map fst (blk_just block).

Definition same_validator_set (left right : list Validator) : Prop :=
  forall validator, In validator left <-> In validator right.

Definition authorized (committee : Committee) (validator : Validator) : Prop :=
  In validator (positive_committee_validators committee).

Definition serialized_post_state_bonds
  (post_state_bonds : BlockHash -> Committee) (block : Block) : Committee :=
  post_state_bonds (blk_hash block).

Definition authority_committee
  (floor_bonds : BlockHash -> Committee)
  (floor_of : Block -> BlockHash)
  (block : Block) : Committee :=
  floor_bonds (floor_of block).

Definition certified_finality_committee
  (floor_bonds : BlockHash -> Committee)
  (floor_active : BlockHash -> list Validator)
  (authority_floor_of : Block -> BlockHash)
  (target : Block) : Committee :=
  active_weight_committee
    (floor_bonds (authority_floor_of target))
    (floor_active (authority_floor_of target)).

Definition FloorStateHash := nat.

Definition certified_state_bound_finality_committee
  (claimed_floor_hash stored_floor_hash : BlockHash)
  (claimed_floor_state stored_floor_state : FloorStateHash)
  (weights : Committee)
  (active : list Validator) : option Committee :=
  if Nat.eq_dec claimed_floor_hash stored_floor_hash then
    if Nat.eq_dec claimed_floor_state stored_floor_state then
      Some (active_weight_committee weights active)
    else None
  else None.

Definition parent_post_state_finality_committee
  (post_state_bonds : BlockHash -> Committee)
  (post_state_active : BlockHash -> list Validator)
  (parent_of : Block -> BlockHash)
  (target : Block) : Committee :=
  active_weight_committee
    (post_state_bonds (parent_of target))
    (post_state_active (parent_of target)).

Definition authority_context_valid
  (floor_bonds : BlockHash -> Committee)
  (floor_of : Block -> BlockHash)
  (block : Block)
  (sender : Validator) : Prop :=
  same_validator_set
    (justification_validators block)
    (positive_committee_validators
      (authority_committee floor_bonds floor_of block))
  /\ authorized (authority_committee floor_bonds floor_of block) sender.

Definition promotion_ready
  (accepted : bool)
  (registered : list Validator)
  (post_state_bonds : BlockHash -> Committee)
  (candidate : BlockHash) : Prop :=
  accepted = true /\
  forall validator,
    In validator (positive_committee_validators (post_state_bonds candidate)) ->
    In validator registered.

Definition register_transition
  (accepted : bool)
  (registered : list Validator)
  (post_state_bonds : BlockHash -> Committee)
  (candidate : BlockHash) : list Validator :=
  if accepted
  then registered ++ positive_committee_validators (post_state_bonds candidate)
  else registered.

Inductive admission_path : Type :=
| ApprovedGenesisAdmission
| OrdinaryReceivedAdmission.

Definition admission_valid
  (canonical_genesis : BlockHash)
  (path : admission_path)
  (block : Block) : Prop :=
  match path with
  | ApprovedGenesisAdmission =>
      blk_hash block = canonical_genesis /\
      blk_num block = 0 /\
      blk_main_parent block = None
  | OrdinaryReceivedAdmission =>
      exists parent, blk_main_parent block = Some parent
  end.

Definition justification_entry_valid
  (sender_of : BlockHash -> Validator)
  (canonical_genesis : BlockHash)
  (genesis_placeholder : Validator)
  (entry : Validator * BlockHash) : Prop :=
  let '(key, cited) := entry in
  key = sender_of cited \/
  (cited = canonical_genesis /\ key = genesis_placeholder).

Definition justification_keys_valid
  (sender_of : BlockHash -> Validator)
  (canonical_genesis : BlockHash)
  (genesis_placeholder : Validator)
  (block : Block) : Prop :=
  Forall
    (justification_entry_valid sender_of canonical_genesis genesis_placeholder)
    (blk_just block).

Definition SlotGenesis := Validator -> option BlockHash.

Inductive GenesisIndex : Type :=
| MissingGenesisIndex
| GenesisAt (hash : BlockHash).

Definition insert_approved_genesis_index
  (canonical_genesis incoming : BlockHash)
  (current : GenesisIndex) : GenesisIndex :=
  if Nat.eq_dec incoming canonical_genesis
  then GenesisAt canonical_genesis
  else current.

Definition seed_registered_genesis
  (accepted : bool)
  (canonical_genesis : BlockHash)
  (current : SlotGenesis)
  (post_state_bonds : BlockHash -> Committee)
  (candidate : BlockHash)
  (validator : Validator) : option BlockHash :=
  if accepted
  then
    if in_dec Nat.eq_dec validator
      (positive_committee_validators (post_state_bonds candidate))
    then Some canonical_genesis
    else current validator
  else current validator.

Definition record_invalid_lmm
  (registered slots : list Validator)
  (sender : Validator) : list Validator :=
  if in_dec Nat.eq_dec sender registered then sender :: slots else slots.

Definition finality_lmm_projection
  (invalid slots : list Validator) : list Validator :=
  filter
    (fun validator =>
       if in_dec Nat.eq_dec validator invalid then false else true)
    slots.

Theorem serialized_bonds_are_post_state_bonds :
  forall post_state_bonds block,
    serialized_post_state_bonds post_state_bonds block =
    post_state_bonds (blk_hash block).
Proof. reflexivity. Qed.

Theorem authority_ignores_same_block_post_state :
  forall floor_bonds floor_of block
    (post_state_bonds_left post_state_bonds_right : BlockHash -> Committee),
    authority_committee floor_bonds floor_of block =
    authority_committee floor_bonds floor_of block.
Proof. reflexivity. Qed.

Theorem same_block_transition_does_not_grant_authority :
  forall floor_bonds post_state_bonds floor_of block validator,
    ~ authorized (authority_committee floor_bonds floor_of block) validator ->
    In validator
      (committee_validators (serialized_post_state_bonds post_state_bonds block)) ->
    ~ authorized (authority_committee floor_bonds floor_of block) validator.
Proof. intros. assumption. Qed.

Theorem certified_finality_committee_is_authority_floor_committee :
  forall floor_bonds floor_active authority_floor_of target,
    certified_finality_committee
      floor_bonds floor_active authority_floor_of target =
    active_weight_committee
      (floor_bonds (authority_floor_of target))
      (floor_active (authority_floor_of target)).
Proof. reflexivity. Qed.

Theorem parent_post_state_transition_does_not_change_certified_finality :
  forall floor_bonds floor_active authority_floor_of target
    (post_state_bonds_left post_state_bonds_right : BlockHash -> Committee)
    (post_state_active_left post_state_active_right : BlockHash -> list Validator)
    (parent_of : Block -> BlockHash),
    certified_finality_committee
      floor_bonds floor_active authority_floor_of target =
    certified_finality_committee
      floor_bonds floor_active authority_floor_of target.
Proof. reflexivity. Qed.

Theorem parent_only_validator_cannot_enter_certified_finality_committee :
  forall floor_bonds floor_active authority_floor_of target validator,
    ~ In validator
      (committee_validators
        (certified_finality_committee
          floor_bonds floor_active authority_floor_of target)) ->
    forall post_state_bonds post_state_active parent_of,
      In validator
        (committee_validators
          (parent_post_state_finality_committee
            post_state_bonds post_state_active parent_of target)) ->
      ~ In validator
        (committee_validators
          (certified_finality_committee
            floor_bonds floor_active authority_floor_of target)).
Proof. intros. assumption. Qed.

Theorem certified_floor_hash_mismatch_fails_closed :
  forall claimed_floor_hash stored_floor_hash
    claimed_floor_state stored_floor_state weights active,
    claimed_floor_hash <> stored_floor_hash ->
    certified_state_bound_finality_committee
      claimed_floor_hash stored_floor_hash
      claimed_floor_state stored_floor_state weights active = None.
Proof.
  intros claimed_floor_hash stored_floor_hash claimed_floor_state
    stored_floor_state weights active Hmismatch.
  unfold certified_state_bound_finality_committee.
  destruct (Nat.eq_dec claimed_floor_hash stored_floor_hash).
  - contradiction.
  - reflexivity.
Qed.

Theorem certified_floor_state_mismatch_fails_closed :
  forall floor_hash claimed_floor_state stored_floor_state weights active,
    claimed_floor_state <> stored_floor_state ->
    certified_state_bound_finality_committee
      floor_hash floor_hash claimed_floor_state stored_floor_state
      weights active = None.
Proof.
  intros floor_hash claimed_floor_state stored_floor_state weights active Hmismatch.
  unfold certified_state_bound_finality_committee.
  destruct (Nat.eq_dec floor_hash floor_hash); [| contradiction].
  destruct (Nat.eq_dec claimed_floor_state stored_floor_state).
  - contradiction.
  - reflexivity.
Qed.

Theorem exact_certified_floor_identity_selects_active_committee :
  forall floor_hash floor_state weights active,
    certified_state_bound_finality_committee
      floor_hash floor_hash floor_state floor_state weights active =
    Some (active_weight_committee weights active).
Proof.
  intros floor_hash floor_state weights active.
  unfold certified_state_bound_finality_committee.
  destruct (Nat.eq_dec floor_hash floor_hash); [| contradiction].
  destruct (Nat.eq_dec floor_state floor_state); [reflexivity | contradiction].
Qed.

Theorem certified_floor_identity_ignores_ambient_committees :
  forall floor_hash floor_state weights active
    (target_committee_left target_committee_right : Committee)
    (head_committee_left head_committee_right : Committee),
    certified_state_bound_finality_committee
      floor_hash floor_hash floor_state floor_state weights active =
    Some (active_weight_committee weights active).
Proof.
  intros.
  apply exact_certified_floor_identity_selects_active_committee.
Qed.

Theorem exact_justifications_are_floor_authorized :
  forall floor_bonds floor_of block,
    same_validator_set
      (justification_validators block)
      (positive_committee_validators
        (authority_committee floor_bonds floor_of block)) ->
    forall validator,
      In validator (justification_validators block) <->
      authorized (authority_committee floor_bonds floor_of block) validator.
Proof.
  intros floor_bonds floor_of block Hexact validator.
  apply Hexact.
Qed.

Theorem valid_authority_context_has_exact_justifications :
  forall floor_bonds floor_of block sender,
    authority_context_valid floor_bonds floor_of block sender ->
    same_validator_set
      (justification_validators block)
      (positive_committee_validators
        (authority_committee floor_bonds floor_of block)).
Proof. intros floor_bonds floor_of block sender [Hexact _]. exact Hexact. Qed.

Theorem valid_authority_context_authorizes_sender :
  forall floor_bonds floor_of block sender,
    authority_context_valid floor_bonds floor_of block sender ->
    authorized (authority_committee floor_bonds floor_of block) sender.
Proof. intros floor_bonds floor_of block sender [_ Hsender]. exact Hsender. Qed.

Theorem accepted_transition_registers_post_state_validators :
  forall registered post_state_bonds candidate validator,
    In validator (positive_committee_validators (post_state_bonds candidate)) ->
    In validator
      (register_transition true registered post_state_bonds candidate).
Proof.
  intros registered post_state_bonds candidate validator Hin.
  simpl. apply in_or_app. right. exact Hin.
Qed.

Theorem ordinary_received_block_has_parent :
  forall canonical_genesis block,
    admission_valid canonical_genesis OrdinaryReceivedAdmission block ->
    exists parent, blk_main_parent block = Some parent.
Proof. intros canonical_genesis block Hadmitted. exact Hadmitted. Qed.

Theorem approved_genesis_is_the_only_admitted_root :
  forall canonical_genesis path block,
    admission_valid canonical_genesis path block ->
    blk_main_parent block = None ->
    path = ApprovedGenesisAdmission /\ blk_hash block = canonical_genesis.
Proof.
  intros canonical_genesis path block Hadmitted Hroot.
  destruct path.
  - simpl in Hadmitted. intuition.
  - simpl in Hadmitted. destruct Hadmitted as [parent Hparent].
    rewrite Hroot in Hparent. discriminate.
Qed.

Theorem counterfeit_root_is_not_admitted :
  forall canonical_genesis path block,
    blk_hash block <> canonical_genesis ->
    blk_main_parent block = None ->
    ~ admission_valid canonical_genesis path block.
Proof.
  intros canonical_genesis path block Hcounterfeit Hroot Hadmitted.
  pose proof
    (approved_genesis_is_the_only_admitted_root
       canonical_genesis path block Hadmitted Hroot) as [_ Hhash].
  contradiction.
Qed.

Theorem non_genesis_justification_key_matches_cited_sender :
  forall sender_of canonical_genesis genesis_placeholder block key cited,
    justification_keys_valid
      sender_of canonical_genesis genesis_placeholder block ->
    In (key, cited) (blk_just block) ->
    cited <> canonical_genesis ->
    key = sender_of cited.
Proof.
  intros sender_of canonical_genesis genesis_placeholder block key cited
    Hvalid Hin Hnon_genesis.
  unfold justification_keys_valid in Hvalid.
  rewrite Forall_forall in Hvalid.
  specialize (Hvalid (key, cited) Hin).
  unfold justification_entry_valid in Hvalid. simpl in Hvalid.
  destruct Hvalid as [Hkey | [Hgenesis _]].
  - exact Hkey.
  - contradiction.
Qed.

Theorem placeholder_justification_cites_only_approved_genesis :
  forall sender_of canonical_genesis genesis_placeholder block cited,
    justification_keys_valid
      sender_of canonical_genesis genesis_placeholder block ->
    In (genesis_placeholder, cited) (blk_just block) ->
    genesis_placeholder <> sender_of cited ->
    cited = canonical_genesis.
Proof.
  intros sender_of canonical_genesis genesis_placeholder block cited
    Hvalid Hin Hnot_sender.
  unfold justification_keys_valid in Hvalid.
  rewrite Forall_forall in Hvalid.
  specialize (Hvalid (genesis_placeholder, cited) Hin).
  unfold justification_entry_valid in Hvalid. simpl in Hvalid.
  destruct Hvalid as [Hsender | [Hgenesis _]].
  - contradiction.
  - exact Hgenesis.
Qed.

Theorem accepted_positive_validator_seeds_canonical_genesis :
  forall canonical_genesis current post_state_bonds candidate validator,
    In validator
      (positive_committee_validators (post_state_bonds candidate)) ->
    seed_registered_genesis true canonical_genesis current
      post_state_bonds candidate validator = Some canonical_genesis.
Proof.
  intros canonical_genesis current post_state_bonds candidate validator Hin.
  unfold seed_registered_genesis. destruct (in_dec Nat.eq_dec validator
    (positive_committee_validators (post_state_bonds candidate))).
  - reflexivity.
  - contradiction.
Qed.

Theorem duplicate_approved_genesis_backfills_legacy_index :
  forall canonical_genesis,
    insert_approved_genesis_index
      canonical_genesis canonical_genesis MissingGenesisIndex =
    GenesisAt canonical_genesis.
Proof.
  intros canonical_genesis. unfold insert_approved_genesis_index.
  destruct (Nat.eq_dec canonical_genesis canonical_genesis); [reflexivity | contradiction].
Qed.

Theorem conflicting_approved_hash_preserves_canonical_index :
  forall canonical_genesis conflicting,
    conflicting <> canonical_genesis ->
    insert_approved_genesis_index
      canonical_genesis conflicting (GenesisAt canonical_genesis) =
    GenesisAt canonical_genesis.
Proof.
  intros canonical_genesis conflicting Hconflict.
  unfold insert_approved_genesis_index.
  destruct (Nat.eq_dec conflicting canonical_genesis); [contradiction | reflexivity].
Qed.

Theorem canonical_genesis_seed_is_independent_of_local_height_zero_order :
  forall canonical_genesis current_left current_right
    post_state_bonds candidate validator,
    In validator
      (positive_committee_validators (post_state_bonds candidate)) ->
    seed_registered_genesis true canonical_genesis current_left
      post_state_bonds candidate validator = Some canonical_genesis /\
    seed_registered_genesis true canonical_genesis current_right
      post_state_bonds candidate validator = Some canonical_genesis.
Proof.
  intros canonical_genesis current_left current_right post_state_bonds
    candidate validator Hin.
  split; apply accepted_positive_validator_seeds_canonical_genesis; exact Hin.
Qed.

Theorem invalid_unregistered_sender_cannot_create_lmm_slot :
  forall registered slots sender,
    ~ In sender registered ->
    record_invalid_lmm registered slots sender = slots.
Proof.
  intros registered slots sender Hunregistered.
  unfold record_invalid_lmm.
  destruct (in_dec Nat.eq_dec sender registered); [contradiction | reflexivity].
Qed.

Theorem accepted_nonpositive_validator_cannot_create_new_slot :
  forall registered post_state_bonds candidate validator,
    ~ In validator registered ->
    ~ In validator
      (positive_committee_validators (post_state_bonds candidate)) ->
    ~ In validator
      (register_transition true registered post_state_bonds candidate).
Proof.
  intros registered post_state_bonds candidate validator
    Hnot_registered Hnot_positive Hin.
  simpl in Hin. apply in_app_or in Hin.
  destruct Hin; contradiction.
Qed.

Theorem invalid_lmm_never_contributes_to_finality_projection :
  forall invalid slots validator,
    In validator invalid ->
    ~ In validator (finality_lmm_projection invalid slots).
Proof.
  intros invalid slots validator Hinvalid Hprojected.
  unfold finality_lmm_projection in Hprojected.
  rewrite filter_In in Hprojected.
  destruct Hprojected as [_ Hkept].
  destruct (in_dec Nat.eq_dec validator invalid); [discriminate | contradiction].
Qed.

Theorem rejected_transition_preserves_registration :
  forall registered post_state_bonds candidate,
    register_transition false registered post_state_bonds candidate = registered.
Proof. reflexivity. Qed.

Theorem rejected_transition_cannot_register_new_validator :
  forall registered post_state_bonds candidate validator,
    ~ In validator registered ->
    ~ In validator
      (register_transition false registered post_state_bonds candidate).
Proof. intros. simpl. assumption. Qed.

Theorem promotion_requires_accepted_registration :
  forall accepted registered post_state_bonds candidate validator,
    promotion_ready accepted registered post_state_bonds candidate ->
    In validator
      (positive_committee_validators (post_state_bonds candidate)) ->
    accepted = true /\ In validator registered.
Proof.
  intros accepted registered post_state_bonds candidate validator
    [Haccepted Hregistered] Hin.
  split; [exact Haccepted | apply Hregistered; exact Hin].
Qed.

Theorem rejected_transition_cannot_promote :
  forall registered post_state_bonds candidate,
    ~ promotion_ready false registered post_state_bonds candidate.
Proof. intros registered post_state_bonds candidate [H _]. discriminate. Qed.

Theorem registered_transition_is_eligible_after_floor_promotion :
  forall floor_bonds post_state_bonds floor_of registered source promoted validator,
    promotion_ready true
      (register_transition true registered post_state_bonds (blk_hash source))
      post_state_bonds
      (blk_hash source) ->
    floor_of promoted = blk_hash source ->
    floor_bonds (blk_hash source) = post_state_bonds (blk_hash source) ->
    In validator
      (positive_committee_validators
        (serialized_post_state_bonds post_state_bonds source)) ->
    authorized (authority_committee floor_bonds floor_of promoted) validator.
Proof.
  intros floor_bonds post_state_bonds floor_of registered source promoted validator
    _ Hfloor Hbonds Hin.
  unfold authorized, authority_committee.
  rewrite Hfloor, Hbonds.
  exact Hin.
Qed.

Theorem active_weight_committee_exact :
  forall weights active bond,
    In bond (active_weight_committee weights active) <->
    In bond weights /\ In (fst bond) active /\ 0 < snd bond.
Proof.
  intros weights active [validator stake].
  unfold active_weight_committee.
  rewrite filter_In.
  simpl.
  split.
  - intros [Hin Hkept].
    split; [exact Hin |].
    destruct (in_dec Nat.eq_dec validator active) as [Hactive | Hinactive].
    + split; [exact Hactive |].
      apply Nat.ltb_lt. exact Hkept.
    + discriminate.
  - intros [Hin [Hactive Hpositive]].
    split; [exact Hin |].
    destruct (in_dec Nat.eq_dec validator active) as [_ | Hinactive].
    + apply Nat.ltb_lt. exact Hpositive.
    + contradiction.
Qed.

Theorem inactive_bond_does_not_change_active_weight_committee :
  forall weights active validator stake,
    ~ In validator active ->
    active_weight_committee ((validator, stake) :: weights) active =
    active_weight_committee weights active.
Proof.
  intros weights active validator stake Hinactive.
  unfold active_weight_committee. simpl.
  destruct (in_dec Nat.eq_dec validator active); [contradiction | reflexivity].
Qed.
