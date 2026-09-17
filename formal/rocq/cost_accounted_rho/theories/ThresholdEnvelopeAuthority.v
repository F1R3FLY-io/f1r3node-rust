From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Sorting.Permutation Lia.
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

Lemma selected_length_bounded : forall members presence,
  length (select_present members presence) <= length members.
Proof.
  induction members as [|member members IH]; intros [|present presence]; cbn;
    try lia.
  destruct present; cbn; specialize (IH presence); lia.
Qed.

Lemma selected_no_duplicates : forall members presence,
  NoDup members -> NoDup (select_present members presence).
Proof.
  induction members as [|member members IH]; intros [|present presence] unique;
    cbn; try constructor.
  inversion unique as [|head tail absent tail_unique]; subst.
  destruct present; cbn.
  - constructor.
    + intro selected. apply absent.
      now apply select_present_member in selected.
    + now apply IH.
  - now apply IH.
Qed.

Lemma unique_ground_projection : forall members,
  NoDup members -> ground_unique members -> NoDup (map ground_of members).
Proof.
  intros members unique. induction unique as [|member members absent unique IH];
    intros ground_unique_members; cbn.
  - constructor.
  - constructor.
    + intros projected. apply in_map_iff in projected.
      destruct projected as [other [same_ground present]].
      apply absent.
      assert (other = member) as equal.
      { apply ground_unique_members; cbn; auto. }
      now subst other.
    + apply IH. intros left right left_in right_in same_ground.
      apply ground_unique_members; cbn; auto.
Qed.

Theorem accepted_funding_has_no_duplicate_ground : forall authorization,
  valid_authorization authorization ->
  NoDup (funding_authorities authorization).
Proof.
  intros authorization valid.
  rewrite funding_is_exact_selected_projection by exact valid.
  destruct valid as [[_ [unique [ground_unique_members _]]] _].
  apply unique_ground_projection.
  - now apply selected_no_duplicates.
  - intros left right left_in right_in same_ground.
    apply select_present_member in left_in.
    apply select_present_member in right_in.
    eapply ground_unique_members; eauto.
Qed.

Definition bounded_authorization (member_cap : nat)
  (authorization : deploy_authorization) : Prop :=
  0 < member_cap /\
  length (policy_members (authorization_policy_value authorization)) <= member_cap /\
  valid_authorization authorization.

Theorem accepted_funding_arity_bounds : forall member_cap authorization,
  bounded_authorization member_cap authorization ->
  1 <= length (funding_authorities authorization) <= member_cap.
Proof.
  intros member_cap authorization [positive [bounded valid]].
  destruct valid as [[nonempty [_ [_ threshold_bounds]]] [_ [witnesses quorum]]].
  unfold funding_authorities. rewrite length_map, witnesses.
  pose proof (selected_length_bounded
    (policy_members (authorization_policy_value authorization))
    (authorization_presence authorization)) as selected_bound.
  unfold quorum_met in quorum.
  destruct (policy_kind_value (authorization_policy_value authorization));
    [destruct (policy_members (authorization_policy_value authorization)); cbn in *|];
    try contradiction; lia.
Qed.

Theorem member_cap_checks_absent_members : forall member_cap authorization,
  member_cap < length (policy_members (authorization_policy_value authorization)) ->
  ~ bounded_authorization member_cap authorization.
Proof.
  intros member_cap authorization over [_ [bounded _]]. lia.
Qed.

Theorem raising_member_cap_preserves_authorization : forall small large authorization,
  small <= large -> bounded_authorization small authorization ->
  bounded_authorization large authorization.
Proof.
  unfold bounded_authorization. intros small large authorization order [positive [bound valid]].
  split; [lia|]. split; [lia|exact valid].
Qed.

Lemma select_paired_present : forall (entries : list (principal * bool)),
  select_present (map fst entries) (map snd entries) =
  map fst (filter snd entries).
Proof.
  induction entries as [|[member present] entries IH]; cbn; auto.
  destruct present; cbn; now rewrite IH.
Qed.

Lemma paired_selection_permutation : forall left right : list (principal * bool),
  Permutation left right ->
  Permutation (map fst (filter snd left)) (map fst (filter snd right)).
Proof.
  intros left right reordered. induction reordered; cbn.
  - constructor.
  - destruct (snd x); cbn; auto using perm_skip.
  - destruct (snd x), (snd y); cbn; auto using perm_swap, Permutation_refl.
  - eapply Permutation_trans; eauto.
Qed.

Theorem paired_input_permutation_preserves_funding :
  forall left right : list (principal * bool),
  Permutation left right ->
  Permutation
    (map ground_of (select_present (map fst left) (map snd left)))
    (map ground_of (select_present (map fst right) (map snd right))).
Proof.
  intros left right reordered.
  rewrite !select_paired_present.
  apply Permutation_map. now apply paired_selection_permutation.
Qed.

Example presence_must_move_with_its_principal :
  select_present [(Secp256k1, 0); (Secp256k1, 1)] [true; false] <>
  select_present [(Secp256k1, 1); (Secp256k1, 0)] [true; false].
Proof.
  discriminate.
Qed.

Definition principal_present (selected : list principal) (member : principal) : bool :=
  if in_dec principal_eq_dec member selected then true else false.

Definition canonical_selection (members selected : list principal) : list principal :=
  filter (principal_present selected) members.

Lemma principal_present_exact : forall selected member,
  principal_present selected member = true <-> In member selected.
Proof.
  intros selected member. unfold principal_present.
  destruct (in_dec principal_eq_dec member selected); intuition discriminate.
Qed.

Theorem canonical_selection_uses_policy_order : forall members selected,
  select_present members (map (principal_present selected) members) =
  canonical_selection members selected.
Proof.
  induction members as [|member members IH]; intros selected; cbn; auto.
  unfold canonical_selection in *. cbn.
  destruct (principal_present selected member); cbn; now rewrite IH.
Qed.

Theorem canonical_selection_permutation_invariant : forall members left right,
  Permutation left right ->
  canonical_selection members left = canonical_selection members right.
Proof.
  intros members left right reordered. unfold canonical_selection.
  apply filter_ext. intros member.
  unfold principal_present.
  destruct (in_dec principal_eq_dec member left) as [in_left|not_left];
    destruct (in_dec principal_eq_dec member right) as [in_right|not_right]; auto.
  - exfalso. apply not_right. eapply Permutation_in; eauto.
  - exfalso. apply not_left. eapply Permutation_in; [apply Permutation_sym; exact reordered|exact in_right].
Qed.

Theorem canonical_selection_preserves_every_selected_member : forall members selected,
  NoDup members -> NoDup selected -> incl selected members ->
  Permutation (canonical_selection members selected) selected.
Proof.
  intros members selected members_unique selected_unique included.
  apply NoDup_Permutation.
  - apply NoDup_filter. exact members_unique.
  - exact selected_unique.
  - intros member. unfold canonical_selection. rewrite filter_In, principal_present_exact.
    split; [tauto|]. intro present. split; auto.
Qed.

Definition construct_authorization (policy : authorization_policy)
  (selected : list principal) : deploy_authorization :=
  {| authorization_policy_value := policy;
     authorization_presence := map (principal_present selected) (policy_members policy);
     authorization_witnesses := canonical_selection (policy_members policy) selected |}.

Theorem canonical_authorization_construction : forall cap policy selected,
  0 < cap -> length (policy_members policy) <= cap ->
  policy_well_formed policy -> NoDup selected ->
  incl selected (policy_members policy) -> quorum_met policy selected ->
  bounded_authorization cap (construct_authorization policy selected).
Proof.
  intros cap policy selected positive bounded well_formed unique included quorum.
  assert (Permutation (canonical_selection (policy_members policy) selected) selected) as reordered.
  { apply canonical_selection_preserves_every_selected_member; try assumption.
    destruct well_formed as [_ [members_unique _]]. exact members_unique. }
  pose proof (Permutation_length reordered) as same_length.
  unfold bounded_authorization. split; [exact positive|]. split; [exact bounded|].
  unfold valid_authorization, construct_authorization; cbn.
  split; [exact well_formed|]. split; [apply length_map|].
  rewrite canonical_selection_uses_policy_order.
  split; [reflexivity|].
  unfold quorum_met in *. destruct (policy_kind_value policy); now rewrite same_length.
Qed.

Theorem canonical_construction_ignores_selected_input_order : forall policy left right,
  Permutation left right ->
  construct_authorization policy left = construct_authorization policy right.
Proof.
  intros policy left right reordered.
  assert (map (principal_present left) (policy_members policy) =
          map (principal_present right) (policy_members policy)) as same_presence.
  { apply map_ext. intros member.
    unfold principal_present.
    destruct (in_dec principal_eq_dec member left) as [in_left|not_left];
      destruct (in_dec principal_eq_dec member right) as [in_right|not_right]; auto.
    - exfalso. apply not_right. eapply Permutation_in; eauto.
    - exfalso. apply not_left. eapply Permutation_in; [apply Permutation_sym; exact reordered|exact in_right]. }
  unfold construct_authorization. rewrite same_presence.
  now rewrite (canonical_selection_permutation_invariant _ _ _ reordered).
Qed.

Definition native_members (keys : list nat) : list principal :=
  map (fun key => (Secp256k1, key)) keys.

Lemma native_members_unique : forall keys,
  NoDup keys -> NoDup (native_members keys).
Proof.
  intros keys unique. induction unique as [|key keys absent unique IH]; cbn.
  - constructor.
  - constructor; [|exact IH].
    intro present. apply in_map_iff in present.
    destruct present as [other [same present]]. inversion same; subst. contradiction.
Qed.

Lemma native_members_ground_unique : forall keys,
  ground_unique (native_members keys).
Proof.
  intros keys left right left_in right_in same_ground.
  apply in_map_iff in left_in. apply in_map_iff in right_in.
  destruct left_in as [left_key [left_equal _]].
  destruct right_in as [right_key [right_equal _]].
  subst left right. unfold ground_of in same_ground. cbn in same_ground.
  now subst right_key.
Qed.

Theorem every_positive_arity_has_allof_authorization : forall cap arity,
  1 <= arity <= cap ->
  exists authorization,
    bounded_authorization cap authorization /\
    length (funding_authorities authorization) = arity.
Proof.
  intros cap arity [positive bounded].
  set (members := native_members (seq 0 arity)).
  set (policy := {| policy_members := members; policy_kind_value := AllOf |}).
  assert (length members = arity) as member_length.
  { unfold members, native_members. rewrite length_map, length_seq. reflexivity. }
  assert (NoDup members) as unique.
  { apply native_members_unique. apply seq_NoDup. }
  assert (ground_unique members) as ground_unique_members.
  { apply native_members_ground_unique. }
  assert (policy_well_formed policy) as well_formed.
  { unfold policy_well_formed, policy; cbn.
    split.
    - intro empty. rewrite empty in member_length. cbn in member_length. lia.
    - split; [exact unique|]. split; [exact ground_unique_members|exact I]. }
  exists (construct_authorization policy members). split.
  - apply canonical_authorization_construction; try assumption; cbn; try lia.
    + unfold incl. auto.
  - unfold funding_authorities, construct_authorization; cbn.
    rewrite length_map.
    rewrite <- member_length.
    apply Permutation_length.
    apply canonical_selection_preserves_every_selected_member; auto.
    unfold incl. auto.
Qed.
