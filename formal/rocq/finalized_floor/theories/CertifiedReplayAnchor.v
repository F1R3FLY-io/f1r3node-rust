From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia.
From FinalizedFloor Require Import StateEffectProvenance.
From FinalizedFloor Require Import FinalityThresholdAlignment.
Import ListNotations.

Section CertifiedReplayAnchor.

Context {Effect Floor : Type}.

Record replay_context := {
  committed_floor : Floor;
  committed_state : @state Effect;
  derived_floor_evidence : Floor
}.

Definition replay_state
  (context : replay_context)
  (visible own : @state Effect)
  : @state Effect :=
  @construct_state Effect (committed_state context) visible own.

Record accepted_replay
  (context : replay_context)
  (visible own pre_state : @state Effect)
  : Prop := {
  accepted_pre_state_exact :
    @state_equiv Effect pre_state (replay_state context visible own)
}.

Record certified_floor_binding
  (context : replay_context)
  (stored_floor : Floor)
  (stored_state : @state Effect)
  : Prop := {
  certified_floor_hash_exact : stored_floor = committed_floor context;
  certified_floor_state_exact :
    @state_equiv Effect stored_state (committed_state context)
}.

Theorem replay_state_preserves_committed_state :
  forall context visible own,
    preserves (committed_state context) (replay_state context visible own).
Proof.
  intros context visible own.
  apply construct_state_preserves_state_parent.
Qed.

Theorem accepted_replay_preserves_committed_state :
  forall context visible own pre_state,
    accepted_replay context visible own pre_state ->
    preserves (committed_state context) pre_state.
Proof.
  intros context visible own pre_state Haccepted effect Hcommitted.
  destruct Haccepted as [Hexact].
  apply (proj2 (Hexact effect)).
  unfold replay_state, construct_state.
  left.
  exact Hcommitted.
Qed.

Theorem derived_floor_evidence_cannot_change_replay :
  forall committed committed_state_value derived_left derived_right visible own,
    state_equiv
      (replay_state
        {| committed_floor := committed;
           committed_state := committed_state_value;
           derived_floor_evidence := derived_left |}
        visible own)
      (replay_state
        {| committed_floor := committed;
           committed_state := committed_state_value;
           derived_floor_evidence := derived_right |}
        visible own).
Proof.
  intros.
  unfold state_equiv, replay_state.
  tauto.
Qed.

Theorem certified_binding_selects_committed_floor_and_state :
  forall context stored_floor stored_state,
    certified_floor_binding context stored_floor stored_state ->
    stored_floor = committed_floor context /\
    state_equiv stored_state (committed_state context).
Proof.
  intros context stored_floor stored_state Hbinding.
  destruct Hbinding.
  split; assumption.
Qed.

Definition certified_replay_anchor_contract : Prop :=
  (forall context visible own,
    preserves (committed_state context) (replay_state context visible own)) /\
  (forall context visible own pre_state,
    accepted_replay context visible own pre_state ->
    preserves (committed_state context) pre_state) /\
  (forall committed committed_state_value derived_left derived_right visible own,
    state_equiv
      (replay_state
        {| committed_floor := committed;
           committed_state := committed_state_value;
           derived_floor_evidence := derived_left |}
        visible own)
      (replay_state
        {| committed_floor := committed;
           committed_state := committed_state_value;
           derived_floor_evidence := derived_right |}
        visible own)) /\
  (forall context stored_floor stored_state,
    certified_floor_binding context stored_floor stored_state ->
    stored_floor = committed_floor context /\
    state_equiv stored_state (committed_state context)).

Theorem certified_replay_anchor_correct : certified_replay_anchor_contract.
Proof.
  exact
    (conj replay_state_preserves_committed_state
      (conj accepted_replay_preserves_committed_state
        (conj derived_floor_evidence_cannot_change_replay
          certified_binding_selects_committed_floor_and_state))).
Qed.

End CertifiedReplayAnchor.

Print Assumptions replay_state_preserves_committed_state.
Print Assumptions accepted_replay_preserves_committed_state.
Print Assumptions derived_floor_evidence_cannot_change_replay.
Print Assumptions certified_binding_selects_committed_floor_and_state.
Print Assumptions certified_replay_anchor_correct.

Section SignedFloorReplayReadiness.

Context {Floor State Digest Validator Generation Effect : Type}.

Variable floor_eq_dec : forall left right : Floor, {left = right} + {left <> right}.
Variable state_eq_dec : forall left right : State, {left = right} + {left <> right}.
Variable digest_eq_dec : forall left right : Digest, {left = right} + {left <> right}.
Variable validator_eq_dec :
  forall left right : Validator, {left = right} + {left <> right}.
Variable generation_eq_dec :
  forall left right : Generation, {left = right} + {left <> right}.
Variable committee_of_state : State -> list Validator.
Variable state_of_floor : Floor -> State.
Variable height_of_floor : Floor -> nat.

Record signed_floor_certificate := {
  certificate_predecessor_floor : Floor;
  certificate_floor : Floor;
  certificate_state : State;
  certificate_height : nat;
  certificate_authority_digest : Digest;
  certificate_ftt_numerator : Z;
  certificate_ftt_denominator : Z;
  certificate_agreeing_stake : Z;
  certificate_clique_stake : Z;
  certificate_total_stake : Z
}.

Variable digest_certificate : signed_floor_certificate -> Digest.

Record signed_floor_commitment := {
  commitment_floor : Floor;
  commitment_state : State;
  commitment_certificate_digest : Digest;
  commitment_authority_digest : Digest
}.

Record stored_floor_occurrence := {
  stored_block_floor : Floor;
  stored_block_state : State;
  stored_block_height : nat;
  stored_metadata_floor : Floor;
  stored_metadata_state : State;
  stored_metadata_height : nat;
  stored_occurrence_accepted : bool
}.

Record certificate_decision_authority := {
  decision_authority_floor : Floor;
  decision_authority_committee : list Validator;
  decision_authority_latest_domain : list Validator;
  decision_authority_digest : Digest
}.

Record proposal_sender_authority := {
  proposal_authority_floor : Floor;
  proposal_authority_committee : list Validator;
  proposal_authority_latest_domain : list Validator;
  proposal_authority_sender : Validator;
  proposal_authority_sender_stake : nat;
  proposal_authority_sender_generation : Generation;
  proposal_authority_claimed_generation : Generation;
  proposal_authority_digest : Digest
}.

Record signed_floor_capture := {
  capture_certificate_available : bool;
  capture_block_available : bool;
  capture_metadata_available : bool;
  capture_state_available : bool;
  capture_certificate_shape_valid : bool;
  capture_certificate_verified : bool;
  capture_protocol_binding_exact : bool;
  capture_shard_binding_exact : bool;
  capture_genesis_binding_exact : bool;
  capture_expected_ftt_numerator : Z;
  capture_expected_ftt_denominator : Z;
  capture_commitment : signed_floor_commitment;
  capture_certificate : signed_floor_certificate;
  capture_occurrence : stored_floor_occurrence;
  capture_parent_descends_from_floor : bool;
  capture_decision_authority : certificate_decision_authority;
  capture_proposal_authority : proposal_sender_authority;
  capture_candidate_floor : Floor;
  capture_committed_effects : @state Effect
}.

Inductive missing_floor_artifact :=
| MissingCertificate
| MissingFloorBlock
| MissingFloorMetadata
| MissingFloorState.

Inductive invalid_floor_reason :=
| InvalidCommitment
| InvalidCertificate
| InvalidOccurrence
| InvalidAncestry
| InvalidAuthority.

Inductive signed_floor_outcome :=
| SignedFloorAccepted
| SignedFloorDeferred (artifact : missing_floor_artifact)
| SignedFloorInvalid (reason : invalid_floor_reason).

Definition floor_eqb (left right : Floor) : bool :=
  if floor_eq_dec left right then true else false.

Definition state_eqb (left right : State) : bool :=
  if state_eq_dec left right then true else false.

Definition digest_eqb (left right : Digest) : bool :=
  if digest_eq_dec left right then true else false.

Definition generation_eqb (left right : Generation) : bool :=
  if generation_eq_dec left right then true else false.

Definition validator_list_eqb (left right : list Validator) : bool :=
  if list_eq_dec validator_eq_dec left right then true else false.

Definition validator_inb (validator : Validator) (validators : list Validator) : bool :=
  if in_dec validator_eq_dec validator validators then true else false.

Definition validator_nodupb (validators : list Validator) : bool :=
  validator_list_eqb (nodup validator_eq_dec validators) validators.

Definition commitment_exactb (capture : signed_floor_capture) : bool :=
  let commitment := capture_commitment capture in
  state_eqb
    (commitment_state commitment)
    (state_of_floor (commitment_floor commitment)).

Definition certificate_ftt_exactb (capture : signed_floor_capture) : bool :=
  let certificate := capture_certificate capture in
  andb
    (Z.eqb
      (certificate_ftt_numerator certificate)
      (capture_expected_ftt_numerator capture))
    (Z.eqb
      (certificate_ftt_denominator certificate)
      (capture_expected_ftt_denominator capture)).

Definition certificate_strict_fttb (capture : signed_floor_capture) : bool :=
  let certificate := capture_certificate capture in
  andb
    (Z.ltb 0 (certificate_total_stake certificate))
    (andb
      (Z.ltb 0 (certificate_ftt_denominator certificate))
      (andb
        (Z.ltb
          (certificate_total_stake certificate)
          (2 * certificate_agreeing_stake certificate))
        (Z.ltb
          (certificate_total_stake certificate *
            (certificate_ftt_denominator certificate +
              certificate_ftt_numerator certificate))
          (2 * certificate_clique_stake certificate *
            certificate_ftt_denominator certificate)))).

Definition certificate_digest_exactb (capture : signed_floor_capture) : bool :=
  digest_eqb
    (commitment_certificate_digest (capture_commitment capture))
    (digest_certificate (capture_certificate capture)).

Definition certificate_exactb (capture : signed_floor_capture) : bool :=
  let commitment := capture_commitment capture in
  let certificate := capture_certificate capture in
  andb
    (floor_eqb (certificate_floor certificate) (commitment_floor commitment))
    (andb
      (state_eqb (certificate_state certificate) (commitment_state commitment))
      (andb
        (Nat.eqb
          (certificate_height certificate)
          (height_of_floor (certificate_floor certificate)))
        (andb
          (capture_protocol_binding_exact capture)
          (andb
            (capture_shard_binding_exact capture)
            (andb
              (capture_genesis_binding_exact capture)
              (andb
                (certificate_ftt_exactb capture)
                (andb
                  (certificate_strict_fttb capture)
                  (certificate_digest_exactb capture)))))))).

Definition occurrence_exactb (capture : signed_floor_capture) : bool :=
  let commitment := capture_commitment capture in
  let occurrence := capture_occurrence capture in
  andb
    (floor_eqb (stored_block_floor occurrence) (commitment_floor commitment))
    (andb
      (floor_eqb (stored_metadata_floor occurrence) (commitment_floor commitment))
      (andb
        (state_eqb (stored_block_state occurrence) (commitment_state commitment))
        (andb
          (state_eqb
            (stored_metadata_state occurrence)
            (commitment_state commitment))
          (andb
            (Nat.eqb
              (stored_block_height occurrence)
              (certificate_height (capture_certificate capture)))
            (andb
              (Nat.eqb
                (stored_metadata_height occurrence)
                (certificate_height (capture_certificate capture)))
              (stored_occurrence_accepted occurrence)))))).

Definition decision_authority_exactb (capture : signed_floor_capture) : bool :=
  let certificate := capture_certificate capture in
  let authority := capture_decision_authority capture in
  andb
    (floor_eqb
      (decision_authority_floor authority)
      (certificate_predecessor_floor certificate))
    (andb
      (validator_nodupb (decision_authority_committee authority))
      (andb
        (validator_list_eqb
          (decision_authority_committee authority)
          (committee_of_state
            (state_of_floor (certificate_predecessor_floor certificate))))
        (andb
          (validator_list_eqb
            (decision_authority_latest_domain authority)
            (decision_authority_committee authority))
          (digest_eqb
            (decision_authority_digest authority)
            (certificate_authority_digest certificate))))).

Definition proposal_authority_exactb (capture : signed_floor_capture) : bool :=
  let commitment := capture_commitment capture in
  let authority := capture_proposal_authority capture in
  andb
    (floor_eqb
      (proposal_authority_floor authority)
      (commitment_floor commitment))
    (andb
      (validator_nodupb (proposal_authority_committee authority))
      (andb
        (validator_list_eqb
          (proposal_authority_committee authority)
          (committee_of_state (commitment_state commitment)))
        (andb
          (validator_list_eqb
            (proposal_authority_latest_domain authority)
            (proposal_authority_committee authority))
          (andb
            (validator_inb
              (proposal_authority_sender authority)
              (proposal_authority_committee authority))
            (andb
              (Nat.ltb 0 (proposal_authority_sender_stake authority))
              (andb
                (generation_eqb
                  (proposal_authority_claimed_generation authority)
                  (proposal_authority_sender_generation authority))
                (digest_eqb
                  (proposal_authority_digest authority)
                  (commitment_authority_digest commitment)))))))).

Definition authority_exactb (capture : signed_floor_capture) : bool :=
  andb
    (decision_authority_exactb capture)
    (proposal_authority_exactb capture).

Definition classify_signed_floor
  (capture : signed_floor_capture)
  : signed_floor_outcome :=
  if negb (capture_certificate_available capture)
  then SignedFloorDeferred MissingCertificate
  else if negb (capture_block_available capture)
       then SignedFloorDeferred MissingFloorBlock
       else if negb (capture_metadata_available capture)
            then SignedFloorDeferred MissingFloorMetadata
            else if negb (capture_state_available capture)
                 then SignedFloorDeferred MissingFloorState
                 else if negb (capture_certificate_shape_valid capture)
                      then SignedFloorInvalid InvalidCertificate
                      else if negb (capture_certificate_verified capture)
                           then SignedFloorInvalid InvalidCertificate
                           else if negb (commitment_exactb capture)
                                then SignedFloorInvalid InvalidCommitment
                                else if negb (certificate_exactb capture)
                                     then SignedFloorInvalid InvalidCertificate
                                     else if negb (occurrence_exactb capture)
                                          then SignedFloorInvalid InvalidOccurrence
                                          else if negb (capture_parent_descends_from_floor capture)
                                               then SignedFloorInvalid InvalidAncestry
                                               else if negb (authority_exactb capture)
                                                    then SignedFloorInvalid InvalidAuthority
                                                    else SignedFloorAccepted.

Definition with_candidate_floor
  (capture : signed_floor_capture)
  (candidate : Floor)
  : signed_floor_capture :=
  {| capture_certificate_available := capture_certificate_available capture;
     capture_block_available := capture_block_available capture;
     capture_metadata_available := capture_metadata_available capture;
     capture_state_available := capture_state_available capture;
     capture_certificate_shape_valid := capture_certificate_shape_valid capture;
     capture_certificate_verified := capture_certificate_verified capture;
     capture_protocol_binding_exact := capture_protocol_binding_exact capture;
     capture_shard_binding_exact := capture_shard_binding_exact capture;
     capture_genesis_binding_exact := capture_genesis_binding_exact capture;
     capture_expected_ftt_numerator := capture_expected_ftt_numerator capture;
     capture_expected_ftt_denominator := capture_expected_ftt_denominator capture;
     capture_commitment := capture_commitment capture;
     capture_certificate := capture_certificate capture;
     capture_occurrence := capture_occurrence capture;
     capture_parent_descends_from_floor := capture_parent_descends_from_floor capture;
     capture_decision_authority := capture_decision_authority capture;
     capture_proposal_authority := capture_proposal_authority capture;
     capture_candidate_floor := candidate;
     capture_committed_effects := capture_committed_effects capture |}.

Theorem floor_eqb_true_iff :
  forall left right, floor_eqb left right = true <-> left = right.
Proof.
  intros left right.
  unfold floor_eqb.
  destruct (floor_eq_dec left right) as [Hequal | Hdifferent].
  - split.
    + intros.
      exact Hequal.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hequal.
      exfalso.
      apply Hdifferent.
      exact Hequal.
Qed.

Theorem state_eqb_true_iff :
  forall left right, state_eqb left right = true <-> left = right.
Proof.
  intros left right.
  unfold state_eqb.
  destruct (state_eq_dec left right) as [Hequal | Hdifferent].
  - split.
    + intros.
      exact Hequal.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hequal.
      exfalso.
      apply Hdifferent.
      exact Hequal.
Qed.

Theorem digest_eqb_true_iff :
  forall left right, digest_eqb left right = true <-> left = right.
Proof.
  intros left right.
  unfold digest_eqb.
  destruct (digest_eq_dec left right) as [Hequal | Hdifferent].
  - split.
    + intros.
      exact Hequal.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hequal.
      exfalso.
      apply Hdifferent.
      exact Hequal.
Qed.

Theorem generation_eqb_true_iff :
  forall left right, generation_eqb left right = true <-> left = right.
Proof.
  intros left right.
  unfold generation_eqb.
  destruct (generation_eq_dec left right) as [Hequal | Hdifferent].
  - split.
    + intros.
      exact Hequal.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hequal.
      exfalso.
      apply Hdifferent.
      exact Hequal.
Qed.

Theorem validator_list_eqb_true_iff :
  forall left right, validator_list_eqb left right = true <-> left = right.
Proof.
  intros left right.
  unfold validator_list_eqb.
  destruct (list_eq_dec validator_eq_dec left right) as [Hequal | Hdifferent].
  - split.
    + intros.
      exact Hequal.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hequal.
      exfalso.
      apply Hdifferent.
      exact Hequal.
Qed.

Theorem validator_inb_true_iff :
  forall validator validators,
    validator_inb validator validators = true <-> In validator validators.
Proof.
  intros validator validators.
  unfold validator_inb.
  destruct (in_dec validator_eq_dec validator validators) as [Hpresent | Habsent].
  - split.
    + intros.
      exact Hpresent.
    + intros.
      reflexivity.
  - split.
    + discriminate.
    + intros Hpresent.
      exfalso.
      apply Habsent.
      exact Hpresent.
Qed.

Theorem classify_signed_floor_accepts_iff :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted <->
    capture_certificate_available capture = true /\
    capture_block_available capture = true /\
    capture_metadata_available capture = true /\
    capture_state_available capture = true /\
    capture_certificate_shape_valid capture = true /\
    capture_certificate_verified capture = true /\
    commitment_exactb capture = true /\
    certificate_exactb capture = true /\
    occurrence_exactb capture = true /\
    capture_parent_descends_from_floor capture = true /\
    authority_exactb capture = true.
Proof.
  intros capture.
  unfold classify_signed_floor.
  destruct (capture_certificate_available capture) eqn:Hcertificate;
  destruct (capture_block_available capture) eqn:Hblock;
  destruct (capture_metadata_available capture) eqn:Hmetadata;
  destruct (capture_state_available capture) eqn:Hstate;
  destruct (capture_certificate_shape_valid capture) eqn:Hshape;
  destruct (capture_certificate_verified capture) eqn:Hverified;
  destruct (commitment_exactb capture) eqn:Hexact_commitment;
  destruct (certificate_exactb capture) eqn:Hexact_certificate;
  destruct (occurrence_exactb capture) eqn:Hexact_occurrence;
  destruct (capture_parent_descends_from_floor capture) eqn:Hancestry;
  destruct (authority_exactb capture) eqn:Hauthority;
  simpl; intuition discriminate.
Qed.

Theorem accepted_capture_has_exact_commitment :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    commitment_exactb capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  tauto.
Qed.

Theorem accepted_capture_has_exact_occurrence :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    occurrence_exactb capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  tauto.
Qed.

Theorem accepted_capture_has_available_state :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    capture_state_available capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  tauto.
Qed.

Theorem accepted_capture_has_exact_authority :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    authority_exactb capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  tauto.
Qed.

Theorem missing_certificate_defers :
  forall capture,
    capture_certificate_available capture = false ->
    classify_signed_floor capture = SignedFloorDeferred MissingCertificate.
Proof.
  intros capture Hmissing.
  unfold classify_signed_floor.
  rewrite Hmissing.
  reflexivity.
Qed.

Theorem missing_block_defers :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = false ->
    classify_signed_floor capture = SignedFloorDeferred MissingFloorBlock.
Proof.
  intros capture Hcertificate Hmissing.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hmissing.
  reflexivity.
Qed.

Theorem missing_metadata_defers :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = false ->
    classify_signed_floor capture = SignedFloorDeferred MissingFloorMetadata.
Proof.
  intros capture Hcertificate Hblock Hmissing.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmissing.
  reflexivity.
Qed.

Theorem missing_state_defers :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = false ->
    classify_signed_floor capture = SignedFloorDeferred MissingFloorState.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hmissing.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hmissing.
  reflexivity.
Qed.

Theorem certificate_mismatch_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = true ->
    certificate_exactb capture = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidCertificate.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified
    Hcommitment_exact Hmismatch.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hstate, Hshape, Hverified,
    Hcommitment_exact, Hmismatch.
  reflexivity.
Qed.

Theorem commitment_mismatch_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidCommitment.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified Hmismatch.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hstate, Hshape, Hverified, Hmismatch.
  reflexivity.
Qed.

Theorem commitment_state_mismatch_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    state_eqb
      (commitment_state (capture_commitment capture))
      (state_of_floor (commitment_floor (capture_commitment capture))) = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidCommitment.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified Hmismatch.
  apply commitment_mismatch_is_invalid; assumption.
Qed.

Theorem certificate_height_mismatch_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = true ->
    Nat.eqb
      (certificate_height (capture_certificate capture))
      (height_of_floor (certificate_floor (capture_certificate capture))) = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidCertificate.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified
    Hcommitment Hmismatch.
  apply certificate_mismatch_is_invalid;
    try assumption.
  unfold certificate_exactb.
  rewrite Hmismatch.
  destruct (floor_eqb
    (certificate_floor (capture_certificate capture))
    (commitment_floor (capture_commitment capture)));
  destruct (state_eqb
    (certificate_state (capture_certificate capture))
    (commitment_state (capture_commitment capture))); reflexivity.
Qed.

Theorem occurrence_mismatch_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = true ->
    certificate_exactb capture = true ->
    occurrence_exactb capture = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidOccurrence.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified
    Hcommitment_exact Hcertificate_exact Hmismatch.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hstate, Hshape, Hverified,
    Hcommitment_exact, Hcertificate_exact, Hmismatch.
  reflexivity.
Qed.

Theorem disconnected_floor_is_invalid :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = true ->
    certificate_exactb capture = true ->
    occurrence_exactb capture = true ->
    capture_parent_descends_from_floor capture = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidAncestry.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified
    Hcommitment_exact Hcertificate_exact Hoccurrence_exact Hdisconnected.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hstate, Hshape, Hverified,
    Hcommitment_exact, Hcertificate_exact, Hoccurrence_exact, Hdisconnected.
  reflexivity.
Qed.

Theorem invalid_authority_is_rejected :
  forall capture,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = true ->
    capture_certificate_shape_valid capture = true ->
    capture_certificate_verified capture = true ->
    commitment_exactb capture = true ->
    certificate_exactb capture = true ->
    occurrence_exactb capture = true ->
    capture_parent_descends_from_floor capture = true ->
    authority_exactb capture = false ->
    classify_signed_floor capture = SignedFloorInvalid InvalidAuthority.
Proof.
  intros capture Hcertificate Hblock Hmetadata Hstate Hshape Hverified
    Hcommitment_exact Hcertificate_exact Hoccurrence_exact Hancestry Hauthority.
  unfold classify_signed_floor.
  rewrite Hcertificate, Hblock, Hmetadata, Hstate, Hshape, Hverified,
    Hcommitment_exact, Hcertificate_exact, Hoccurrence_exact, Hancestry, Hauthority.
  reflexivity.
Qed.

Theorem candidate_evidence_cannot_replace_signed_floor :
  forall capture candidate,
    classify_signed_floor (with_candidate_floor capture candidate) =
    classify_signed_floor capture.
Proof.
  intros capture candidate.
  reflexivity.
Qed.

Theorem finalizer_promotion_preserves_frozen_capture :
  forall capture candidate,
    capture_commitment (with_candidate_floor capture candidate) =
      capture_commitment capture /\
    capture_certificate (with_candidate_floor capture candidate) =
      capture_certificate capture /\
    capture_occurrence (with_candidate_floor capture candidate) =
      capture_occurrence capture /\
    capture_decision_authority (with_candidate_floor capture candidate) =
      capture_decision_authority capture /\
    capture_proposal_authority (with_candidate_floor capture candidate) =
      capture_proposal_authority capture /\
    classify_signed_floor (with_candidate_floor capture candidate) =
      classify_signed_floor capture.
Proof.
  intros capture candidate.
  repeat split; reflexivity.
Qed.

Theorem accepted_capture_uses_predecessor_decision_authority :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    decision_authority_exactb capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  destruct Haccepted as [_ [_ [_ [_ [_ [_ [_ [_ [_ [_ Hauthority]]]]]]]]]].
  unfold authority_exactb in Hauthority.
  apply andb_true_iff in Hauthority.
  tauto.
Qed.

Theorem accepted_capture_uses_target_proposal_authority :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    proposal_authority_exactb capture = true.
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  destruct Haccepted as [_ [_ [_ [_ [_ [_ [_ [_ [_ [_ Hauthority]]]]]]]]]].
  unfold authority_exactb in Hauthority.
  apply andb_true_iff in Hauthority.
  tauto.
Qed.

Theorem distinct_authority_contexts_are_accepted_independently :
  forall capture,
    decision_authority_exactb capture = true ->
    proposal_authority_exactb capture = true ->
    certificate_authority_digest (capture_certificate capture) <>
      commitment_authority_digest (capture_commitment capture) ->
    authority_exactb capture = true /\
    certificate_authority_digest (capture_certificate capture) <>
      commitment_authority_digest (capture_commitment capture).
Proof.
  intros capture Hdecision Hproposal Hdifferent.
  split.
  - unfold authority_exactb.
    rewrite Hdecision, Hproposal.
    reflexivity.
  - exact Hdifferent.
Qed.

Theorem accepted_certificate_digest_binds_target_height :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    commitment_certificate_digest (capture_commitment capture) =
      digest_certificate (capture_certificate capture) /\
    certificate_height (capture_certificate capture) =
      height_of_floor (certificate_floor (capture_certificate capture)).
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  destruct Haccepted as
    [_ [_ [_ [_ [_ [_ [_ [Hexact _]]]]]]]].
  unfold certificate_exactb in Hexact.
  repeat rewrite andb_true_iff in Hexact.
  destruct Hexact as [_ [_ [Hheight [_ [_ [_ [_ [_ Hdigest]]]]]]]].
  split.
  - apply digest_eqb_true_iff.
    exact Hdigest.
  - apply Nat.eqb_eq.
    exact Hheight.
Qed.

Theorem accepted_certificate_uses_strict_ftt :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    candidate_floor_certificate
      (certificate_clique_stake (capture_certificate capture))
      (certificate_total_stake (capture_certificate capture))
      (certificate_ftt_numerator (capture_certificate capture))
      (certificate_ftt_denominator (capture_certificate capture)).
Proof.
  intros capture Haccepted.
  apply classify_signed_floor_accepts_iff in Haccepted.
  destruct Haccepted as
    [_ [_ [_ [_ [_ [_ [_ [Hexact _]]]]]]]].
  unfold certificate_exactb in Hexact.
  repeat rewrite andb_true_iff in Hexact.
  destruct Hexact as [_ [_ [_ [_ [_ [_ [_ [Hstrict _]]]]]]]].
  unfold certificate_strict_fttb in Hstrict.
  repeat rewrite andb_true_iff in Hstrict.
  destruct Hstrict as [_ [_ [_ Hftt]]].
  apply Z.ltb_lt in Hftt.
  unfold candidate_floor_certificate, FtExact.ft_exact_gt.
  lia.
Qed.

Theorem accepted_certificate_rejects_inclusive_ftt_boundary :
  forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    (2 * certificate_clique_stake (capture_certificate capture) *
        certificate_ftt_denominator (capture_certificate capture))%Z <>
      (certificate_total_stake (capture_certificate capture) *
        (certificate_ftt_denominator (capture_certificate capture) +
          certificate_ftt_numerator (capture_certificate capture)))%Z.
Proof.
  intros capture Haccepted Hequal.
  pose proof (accepted_certificate_uses_strict_ftt capture Haccepted) as Hstrict.
  unfold candidate_floor_certificate, FtExact.ft_exact_gt in Hstrict.
  lia.
Qed.

Theorem accepted_replay_preserves_signed_floor_effects :
  forall capture visible own,
    classify_signed_floor capture = SignedFloorAccepted ->
    preserves
      (capture_committed_effects capture)
      (construct_state (capture_committed_effects capture) visible own).
Proof.
  intros capture visible own _.
  apply construct_state_preserves_state_parent.
Qed.

Definition signed_floor_replay_readiness_contract : Prop :=
  (forall capture,
    classify_signed_floor capture = SignedFloorAccepted ->
    commitment_exactb capture = true /\
    certificate_exactb capture = true /\
    occurrence_exactb capture = true /\
    capture_state_available capture = true /\
    capture_parent_descends_from_floor capture = true /\
    decision_authority_exactb capture = true /\
    proposal_authority_exactb capture = true /\
    certificate_digest_exactb capture = true /\
    certificate_strict_fttb capture = true) /\
  (forall capture candidate,
    classify_signed_floor (with_candidate_floor capture candidate) =
    classify_signed_floor capture) /\
  (forall capture visible own,
    classify_signed_floor capture = SignedFloorAccepted ->
    preserves
      (capture_committed_effects capture)
      (construct_state (capture_committed_effects capture) visible own)).

Theorem signed_floor_replay_readiness_correct :
  signed_floor_replay_readiness_contract.
Proof.
  unfold signed_floor_replay_readiness_contract.
  split.
  - intros capture Haccepted.
    pose proof Haccepted as Haccepted_exact.
    apply classify_signed_floor_accepts_iff in Haccepted_exact.
    repeat split.
    + apply accepted_capture_has_exact_commitment.
      exact Haccepted.
    + tauto.
    + apply accepted_capture_has_exact_occurrence.
      exact Haccepted.
    + apply accepted_capture_has_available_state.
      exact Haccepted.
    + tauto.
    + apply accepted_capture_uses_predecessor_decision_authority.
      exact Haccepted.
    + apply accepted_capture_uses_target_proposal_authority.
      exact Haccepted.
    + destruct Haccepted_exact as
        [_ [_ [_ [_ [_ [_ [_ [Hexact _]]]]]]]].
      unfold certificate_exactb in Hexact.
      repeat rewrite andb_true_iff in Hexact.
      tauto.
    + destruct Haccepted_exact as
        [_ [_ [_ [_ [_ [_ [_ [Hexact _]]]]]]]].
      unfold certificate_exactb in Hexact.
      repeat rewrite andb_true_iff in Hexact.
      tauto.
  - split.
    + intros capture candidate.
      apply candidate_evidence_cannot_replace_signed_floor.
    + intros capture visible own Haccepted.
      apply accepted_replay_preserves_signed_floor_effects.
      exact Haccepted.
Qed.

End SignedFloorReplayReadiness.

Print Assumptions classify_signed_floor_accepts_iff.
Print Assumptions accepted_capture_has_exact_commitment.
Print Assumptions missing_certificate_defers.
Print Assumptions missing_block_defers.
Print Assumptions missing_metadata_defers.
Print Assumptions missing_state_defers.
Print Assumptions certificate_mismatch_is_invalid.
Print Assumptions commitment_mismatch_is_invalid.
Print Assumptions commitment_state_mismatch_is_invalid.
Print Assumptions certificate_height_mismatch_is_invalid.
Print Assumptions occurrence_mismatch_is_invalid.
Print Assumptions disconnected_floor_is_invalid.
Print Assumptions invalid_authority_is_rejected.
Print Assumptions candidate_evidence_cannot_replace_signed_floor.
Print Assumptions finalizer_promotion_preserves_frozen_capture.
Print Assumptions accepted_capture_uses_predecessor_decision_authority.
Print Assumptions accepted_capture_uses_target_proposal_authority.
Print Assumptions distinct_authority_contexts_are_accepted_independently.
Print Assumptions accepted_certificate_digest_binds_target_height.
Print Assumptions accepted_certificate_uses_strict_ftt.
Print Assumptions accepted_certificate_rejects_inclusive_ftt_boundary.
Print Assumptions signed_floor_replay_readiness_correct.
