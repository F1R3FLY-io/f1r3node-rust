From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Inductive signature_scheme :=
| Secp256k1
| Secp256k1Eth.

Definition signature_scheme_eq_dec :
  forall left right : signature_scheme, {left = right} + {left <> right}.
Proof.
  decide equality.
Defined.

Definition principal := (signature_scheme * nat)%type.
Definition ground_authority := nat.

Definition principal_eq_dec :
  forall left right : principal, {left = right} + {left <> right}.
Proof.
  decide equality; apply signature_scheme_eq_dec || apply Nat.eq_dec.
Defined.

Definition ground_of (value : principal) : ground_authority := snd value.

Inductive policy_kind :=
| AllOf
| Threshold (minimum : nat).

Record authorization_policy := {
  policy_members : list principal;
  policy_kind_value : policy_kind
}.

Fixpoint select_present
  (members : list principal)
  (presence : list bool)
  : list principal :=
  match members, presence with
  | member :: remaining_members, true :: remaining_presence =>
      member :: select_present remaining_members remaining_presence
  | _ :: remaining_members, false :: remaining_presence =>
      select_present remaining_members remaining_presence
  | _, _ => []
  end.

Definition ground_unique (members : list principal) : Prop :=
  forall left right,
    In left members ->
    In right members ->
    ground_of left = ground_of right ->
    left = right.

Definition policy_well_formed (policy : authorization_policy) : Prop :=
  policy_members policy <> [] /\
  NoDup (policy_members policy) /\
  ground_unique (policy_members policy) /\
  match policy_kind_value policy with
  | AllOf => True
  | Threshold minimum =>
      1 <= minimum < length (policy_members policy)
  end.

Definition quorum_met
  (policy : authorization_policy)
  (selected : list principal)
  : Prop :=
  match policy_kind_value policy with
  | AllOf => length selected = length (policy_members policy)
  | Threshold minimum => minimum <= length selected
  end.

Record deploy_authorization := {
  authorization_policy_value : authorization_policy;
  authorization_presence : list bool;
  authorization_witnesses : list principal
}.

Definition valid_authorization (authorization : deploy_authorization) : Prop :=
  let policy := authorization_policy_value authorization in
  let selected :=
    select_present
      (policy_members policy)
      (authorization_presence authorization) in
  policy_well_formed policy /\
  length (authorization_presence authorization) =
    length (policy_members policy) /\
  authorization_witnesses authorization = selected /\
  quorum_met policy selected.

Definition funding_authorities
  (authorization : deploy_authorization)
  : list ground_authority :=
  map ground_of (authorization_witnesses authorization).

Definition runtime_authorities
  (authorization : deploy_authorization)
  : list ground_authority :=
  funding_authorities authorization.

Record deploy_commitment_preimage := {
  commitment_intent : nat;
  commitment_policy : authorization_policy;
  commitment_presence : list bool
}.

Definition preimage_of
  (intent : nat)
  (authorization : deploy_authorization)
  : deploy_commitment_preimage :=
  {| commitment_intent := intent;
     commitment_policy := authorization_policy_value authorization;
     commitment_presence := authorization_presence authorization |}.

Definition authority_projection
  (preimage : deploy_commitment_preimage)
  : list ground_authority :=
  map ground_of
    (select_present
      (policy_members (commitment_policy preimage))
      (commitment_presence preimage)).

Lemma select_present_member :
  forall members presence member,
    In member (select_present members presence) ->
    In member members.
Proof.
  intros members presence.
  revert members.
  induction presence as [|present remaining_presence IH];
    intros members member selected;
    destruct members as [|head remaining_members]; cbn in *.
  - contradiction.
  - contradiction.
  - contradiction.
  - destruct present.
    + destruct selected as [same | selected].
      * now left.
      * right. now apply IH.
    + right. now apply IH.
Qed.

Theorem witnesses_are_exactly_selected :
  forall authorization,
    valid_authorization authorization ->
    authorization_witnesses authorization =
      select_present
        (policy_members (authorization_policy_value authorization))
        (authorization_presence authorization).
Proof.
  intros authorization valid.
  unfold valid_authorization in valid.
  tauto.
Qed.

Theorem every_witness_is_a_policy_member :
  forall authorization witness,
    valid_authorization authorization ->
    In witness (authorization_witnesses authorization) ->
    In witness (policy_members (authorization_policy_value authorization)).
Proof.
  intros authorization witness valid in_witnesses.
  rewrite witnesses_are_exactly_selected in in_witnesses by exact valid.
  now apply select_present_member in in_witnesses.
Qed.

Theorem funding_is_exact_selected_projection :
  forall authorization,
    valid_authorization authorization ->
    funding_authorities authorization =
      map ground_of
        (select_present
          (policy_members (authorization_policy_value authorization))
          (authorization_presence authorization)).
Proof.
  intros authorization valid.
  unfold funding_authorities.
  now rewrite witnesses_are_exactly_selected by exact valid.
Qed.

Theorem unsigned_policy_member_is_not_funded :
  forall authorization member,
    valid_authorization authorization ->
    In member (policy_members (authorization_policy_value authorization)) ->
    ~ In member (authorization_witnesses authorization) ->
    ~ In (ground_of member) (funding_authorities authorization).
Proof.
  intros authorization member valid member_in_policy member_unsigned funded.
  unfold funding_authorities in funded.
  apply in_map_iff in funded.
  destruct funded as [witness [same_ground witness_selected]].
  pose proof valid as valid_copy.
  destruct valid as [well_formed _].
  destruct well_formed as [_ [_ [unique_ground _]]].
  assert (witness_in_policy :
    In witness (policy_members (authorization_policy_value authorization))).
  {
    apply every_witness_is_a_policy_member with (authorization := authorization).
    - exact valid_copy.
    - exact witness_selected.
  }
  apply member_unsigned.
  assert (witness = member) as same_witness.
  {
    apply unique_ground.
    - exact witness_in_policy.
    - exact member_in_policy.
    - exact same_ground.
  }
  now subst witness.
Qed.

Theorem native_and_ethereum_schemes_share_ground_authority :
  forall key,
    ground_of (Secp256k1, key) = ground_of (Secp256k1Eth, key).
Proof.
  reflexivity.
Qed.

Theorem native_and_ethereum_principals_are_distinct :
  forall key : nat,
    (Secp256k1, key) <> (Secp256k1Eth, key).
Proof.
  intros key equal.
  discriminate equal.
Qed.

Theorem policy_rejects_duplicate_ground_owner :
  forall (members : list principal) (key : nat),
    ground_unique members ->
    In (Secp256k1, key) members ->
    In (Secp256k1Eth, key) members ->
    False.
Proof.
  intros members key unique native_present ethereum_present.
  pose proof
    (unique
      (Secp256k1, key)
      (Secp256k1Eth, key)
      native_present
      ethereum_present
      eq_refl) as equal.
  discriminate equal.
Qed.

Theorem commitment_preimage_binds_presence :
  forall intent left right,
    preimage_of intent left = preimage_of intent right ->
    authorization_presence left = authorization_presence right.
Proof.
  intros intent left right equal.
  exact (f_equal commitment_presence equal).
Qed.

Theorem commitment_preimage_binds_policy :
  forall intent left right,
    preimage_of intent left = preimage_of intent right ->
    authorization_policy_value left = authorization_policy_value right.
Proof.
  intros intent left right equal.
  exact (f_equal commitment_policy equal).
Qed.

Theorem different_presence_changes_commitment_preimage :
  forall intent left right,
    authorization_presence left <> authorization_presence right ->
    preimage_of intent left <> preimage_of intent right.
Proof.
  intros intent left right different equal.
  apply different.
  now apply commitment_preimage_binds_presence with (intent := intent).
Qed.

Theorem equal_commitment_preimage_has_equal_authority_projection :
  forall left right,
    left = right ->
    authority_projection left = authority_projection right.
Proof.
  intros left right equal.
  now subst right.
Qed.

Theorem runtime_authority_equals_funding_authority :
  forall authorization,
    runtime_authorities authorization = funding_authorities authorization.
Proof.
  reflexivity.
Qed.

Theorem accepted_threshold_meets_quorum :
  forall authorization minimum,
    valid_authorization authorization ->
    policy_kind_value (authorization_policy_value authorization) =
      Threshold minimum ->
    minimum <= length (authorization_witnesses authorization).
Proof.
  intros authorization minimum valid threshold_kind.
  unfold valid_authorization in valid.
  destruct valid as [_ [_ [witnesses_equal quorum]]].
  rewrite witnesses_equal.
  unfold quorum_met in quorum.
  rewrite threshold_kind in quorum.
  exact quorum.
Qed.
