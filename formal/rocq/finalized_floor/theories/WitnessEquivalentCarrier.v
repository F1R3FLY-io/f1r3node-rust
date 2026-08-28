From Stdlib Require Import Lists.List.

Section WitnessEquivalentCarrier.

Context {Block Digest Floor State Protocol : Type}.

Variable accepted causal : Block -> Prop.

Record carrier : Type := {
  carrier_block : Block;
  carrier_digest : Digest;
  carrier_floor : Floor;
  carrier_state : State;
  carrier_protocol : Protocol
}.

Definition carrier_pair (proof : carrier) : Block * Digest :=
  (carrier_block proof, carrier_digest proof).

Definition valid_carrier
  (expected_protocol : Protocol)
  (expected_floor : Floor)
  (expected_state : State)
  (proof : carrier)
  : Prop :=
  accepted (carrier_block proof) /\
  causal (carrier_block proof) /\
  carrier_protocol proof = expected_protocol /\
  carrier_floor proof = expected_floor /\
  carrier_state proof = expected_state.

Definition receiver_compatible
  (expected_protocol : Protocol)
  (expected_floor : Floor)
  (expected_state : State)
  (_local_witness_digest : Digest)
  (proof : carrier)
  : Prop :=
  valid_carrier expected_protocol expected_floor expected_state proof.

Theorem receiver_compatibility_is_witness_irrelevant :
  forall expected_protocol expected_floor expected_state
    left_digest right_digest proof,
    receiver_compatible
      expected_protocol expected_floor expected_state left_digest proof <->
    receiver_compatible
      expected_protocol expected_floor expected_state right_digest proof.
Proof.
  intros.
  reflexivity.
Qed.

Theorem equivalent_carriers_interoperate_across_local_witnesses :
  forall expected_protocol expected_floor expected_state
    left_digest right_digest left right,
    valid_carrier expected_protocol expected_floor expected_state left ->
    valid_carrier expected_protocol expected_floor expected_state right ->
    receiver_compatible
      expected_protocol expected_floor expected_state left_digest right /\
    receiver_compatible
      expected_protocol expected_floor expected_state right_digest left.
Proof.
  intros expected_protocol expected_floor expected_state
    left_digest right_digest left right left_valid right_valid.
  split; assumption.
Qed.

Theorem selected_pair_binds_exact_carrier_content :
  forall proof selected_block selected_digest,
    carrier_pair proof = (selected_block, selected_digest) ->
    selected_block = carrier_block proof /\
    selected_digest = carrier_digest proof.
Proof.
  intros proof selected_block selected_digest paired.
  unfold carrier_pair in paired.
  inversion paired.
  auto.
Qed.

Theorem cross_carrier_digest_substitution_requires_digest_equality :
  forall selected substituted,
    carrier_pair selected =
      (carrier_block selected, carrier_digest substituted) ->
    carrier_digest selected = carrier_digest substituted.
Proof.
  intros selected substituted paired.
  unfold carrier_pair in paired.
  inversion paired.
  reflexivity.
Qed.

Definition semantic_wake_eligible
  (expected_protocol : Protocol)
  (expected_floor : Floor)
  (expected_state : State)
  (proof : carrier)
  : Prop :=
  valid_carrier expected_protocol expected_floor expected_state proof.

Theorem semantic_wake_is_witness_irrelevant :
  forall expected_protocol expected_floor expected_state
    left_digest right_digest proof,
    receiver_compatible
      expected_protocol expected_floor expected_state left_digest proof ->
    receiver_compatible
      expected_protocol expected_floor expected_state right_digest proof /\
    semantic_wake_eligible
      expected_protocol expected_floor expected_state proof.
Proof.
  intros expected_protocol expected_floor expected_state
    left_digest right_digest proof compatible.
  split; exact compatible.
Qed.

Theorem state_substitution_is_rejected :
  forall expected_protocol expected_floor expected_state substituted_state proof,
    expected_state <> substituted_state ->
    valid_carrier expected_protocol expected_floor substituted_state proof ->
    ~ valid_carrier expected_protocol expected_floor expected_state proof.
Proof.
  intros expected_protocol expected_floor expected_state substituted_state
    proof different [_ [_ [_ [_ substituted]]]].
  intros [_ [_ [_ [_ expected]]]].
  rewrite expected in substituted.
  contradiction.
Qed.

Theorem witness_equivalent_carrier_contract :
  (forall expected_protocol expected_floor expected_state
    left_digest right_digest proof,
    receiver_compatible
      expected_protocol expected_floor expected_state left_digest proof <->
    receiver_compatible
      expected_protocol expected_floor expected_state right_digest proof)
  /\
  (forall expected_protocol expected_floor expected_state
    left_digest right_digest left right,
    valid_carrier expected_protocol expected_floor expected_state left ->
    valid_carrier expected_protocol expected_floor expected_state right ->
    receiver_compatible
      expected_protocol expected_floor expected_state left_digest right /\
    receiver_compatible
      expected_protocol expected_floor expected_state right_digest left)
  /\
  (forall proof selected_block selected_digest,
    carrier_pair proof = (selected_block, selected_digest) ->
    selected_block = carrier_block proof /\
    selected_digest = carrier_digest proof)
  /\
  (forall selected substituted,
    carrier_pair selected =
      (carrier_block selected, carrier_digest substituted) ->
    carrier_digest selected = carrier_digest substituted)
  /\
  (forall expected_protocol expected_floor expected_state substituted_state proof,
    expected_state <> substituted_state ->
    valid_carrier expected_protocol expected_floor substituted_state proof ->
    ~ valid_carrier expected_protocol expected_floor expected_state proof).
Proof.
  split.
  - exact receiver_compatibility_is_witness_irrelevant.
  - split.
    + exact equivalent_carriers_interoperate_across_local_witnesses.
    + split.
      * exact selected_pair_binds_exact_carrier_content.
      * split.
        -- exact cross_carrier_digest_substitution_requires_digest_equality.
        -- exact state_substitution_is_rejected.
Qed.

End WitnessEquivalentCarrier.

Print Assumptions witness_equivalent_carrier_contract.
