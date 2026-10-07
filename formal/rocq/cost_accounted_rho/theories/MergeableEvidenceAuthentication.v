From Stdlib Require Import Arith.PeanoNat.

Record execution_identity := {
  identity_pre_state : nat;
  identity_post_state : nat;
  identity_creator : nat;
  identity_sequence : nat;
  identity_payload : nat
}.

Definition execution_identity_eq_dec :
  forall left right : execution_identity, {left = right} + {left <> right}.
Proof.
  decide equality; apply Nat.eq_dec.
Defined.

Definition mergeable_key := execution_identity.

Definition legacy_key (identity : execution_identity) : nat * nat * nat :=
  (identity_post_state identity,
   identity_creator identity,
   identity_sequence identity).

Definition evidence := nat.
Definition evidence_store := mergeable_key -> option evidence.

Record retirement_observation := {
  retirement_finalized : bool;
  retirement_beyond_horizon : bool;
  retirement_children_known : bool;
  retirement_has_child : bool;
  retirement_has_latest : bool;
  retirement_all_latest_advanced : bool
}.

Definition retirement_eligible (observation : retirement_observation) : bool :=
  retirement_finalized observation &&
  retirement_beyond_horizon observation &&
  retirement_children_known observation &&
  retirement_has_child observation &&
  retirement_has_latest observation &&
  retirement_all_latest_advanced observation.

Definition vacuous_latest_retirement_eligible
  (observation : retirement_observation)
  : bool :=
  retirement_finalized observation &&
  retirement_beyond_horizon observation &&
  retirement_children_known observation &&
  retirement_has_child observation &&
  retirement_all_latest_advanced observation.

Inductive parent_path :=
| MainParentPath
| SecondaryParentPath.

Definition full_dag_recognizes_advancement (_ : parent_path) : bool := true.

Definition main_spine_recognizes_advancement (path : parent_path) : bool :=
  match path with
  | MainParentPath => true
  | SecondaryParentPath => false
  end.

Definition retirement_with_path
  (recognizes_advancement : parent_path -> bool)
  (path : parent_path)
  : bool :=
  retirement_eligible
    {| retirement_finalized := true;
       retirement_beyond_horizon := true;
       retirement_children_known := true;
       retirement_has_child := true;
       retirement_has_latest := true;
       retirement_all_latest_advanced := recognizes_advancement path |}.

Definition empty_evidence_store : evidence_store := fun _ => None.

Definition insert_evidence
  (key : mergeable_key)
  (value : evidence)
  (store : evidence_store)
  : evidence_store :=
  fun query =>
    if execution_identity_eq_dec query key then Some value else store query.

Definition delete_evidence
  (key : mergeable_key)
  (store : evidence_store)
  : evidence_store :=
  fun query =>
    if execution_identity_eq_dec query key then None else store query.

Inductive evidence_source :=
| LocalReplay
| PeerResponse.

Definition publish_evidence
  (source : evidence_source)
  (key : mergeable_key)
  (value : evidence)
  (store : evidence_store)
  : evidence_store :=
  match source with
  | LocalReplay => insert_evidence key value store
  | PeerResponse => store
  end.

Theorem complete_key_is_injective :
  forall left right : execution_identity,
    (left : mergeable_key) = (right : mergeable_key) ->
    left = right.
Proof.
  intros left right equal.
  exact equal.
Qed.

Theorem distinct_pre_states_have_distinct_keys :
  forall left right : execution_identity,
    identity_pre_state left <> identity_pre_state right ->
    (left : mergeable_key) <> (right : mergeable_key).
Proof.
  intros left right pre_diff key_equal.
  apply pre_diff.
  now rewrite key_equal.
Qed.

Theorem distinct_post_states_have_distinct_keys :
  forall left right : execution_identity,
    identity_post_state left <> identity_post_state right ->
    (left : mergeable_key) <> (right : mergeable_key).
Proof.
  intros left right post_diff key_equal.
  apply post_diff.
  now rewrite key_equal.
Qed.

Theorem distinct_creators_have_distinct_keys :
  forall left right : execution_identity,
    identity_creator left <> identity_creator right ->
    (left : mergeable_key) <> (right : mergeable_key).
Proof.
  intros left right creator_diff key_equal.
  apply creator_diff.
  now rewrite key_equal.
Qed.

Theorem distinct_sequences_have_distinct_keys :
  forall left right : execution_identity,
    identity_sequence left <> identity_sequence right ->
    (left : mergeable_key) <> (right : mergeable_key).
Proof.
  intros left right sequence_diff key_equal.
  apply sequence_diff.
  now rewrite key_equal.
Qed.

Theorem distinct_payloads_have_distinct_keys :
  forall left right : execution_identity,
    identity_payload left <> identity_payload right ->
    (left : mergeable_key) <> (right : mergeable_key).
Proof.
  intros left right payload_diff key_equal.
  apply payload_diff.
  now rewrite key_equal.
Qed.

Theorem legacy_key_alias_witness :
  exists left right : execution_identity,
    left <> right /\ legacy_key left = legacy_key right.
Proof.
  exists
    {| identity_pre_state := 0;
       identity_post_state := 2;
       identity_creator := 3;
       identity_sequence := 4;
       identity_payload := 5 |}.
  exists
    {| identity_pre_state := 1;
       identity_post_state := 2;
       identity_creator := 3;
       identity_sequence := 4;
       identity_payload := 6 |}.
  split.
  - intros equal.
    discriminate (f_equal identity_pre_state equal).
  - reflexivity.
Qed.

Theorem local_replay_publishes_exact_evidence :
  forall store key value,
    publish_evidence LocalReplay key value store key = Some value.
Proof.
  intros store key value.
  unfold publish_evidence, insert_evidence.
  destruct (execution_identity_eq_dec key key); congruence.
Qed.

Theorem peer_response_cannot_publish_evidence :
  forall store key value query,
    publish_evidence PeerResponse key value store query = store query.
Proof.
  reflexivity.
Qed.

Theorem peer_response_cannot_overwrite_local_replay :
  forall store key local_value peer_value,
    publish_evidence PeerResponse key peer_value
      (publish_evidence LocalReplay key local_value store) key =
    Some local_value.
Proof.
  intros store key local_value peer_value.
  apply local_replay_publishes_exact_evidence.
Qed.

Theorem distinct_replays_preserve_both_entries :
  forall store left right left_value right_value,
    left <> right ->
    insert_evidence right right_value
      (insert_evidence left left_value store) left = Some left_value /\
    insert_evidence right right_value
      (insert_evidence left left_value store) right = Some right_value.
Proof.
  intros store left right left_value right_value distinct.
  unfold insert_evidence.
  destruct (execution_identity_eq_dec left right) as [equal | not_equal].
  - contradiction.
  - destruct (execution_identity_eq_dec left left); try contradiction.
    destruct (execution_identity_eq_dec right right); try contradiction.
    split; reflexivity.
Qed.

Theorem distinct_insertions_commute_pointwise :
  forall store left right left_value right_value query,
    left <> right ->
    insert_evidence right right_value
      (insert_evidence left left_value store) query =
    insert_evidence left left_value
      (insert_evidence right right_value store) query.
Proof.
  intros store left right left_value right_value query distinct.
  unfold insert_evidence.
  destruct (execution_identity_eq_dec query right) as [query_right | query_not_right].
  - subst query.
    destruct (execution_identity_eq_dec right left) as [equal | not_equal].
    + symmetry in equal; contradiction.
    + reflexivity.
  - destruct (execution_identity_eq_dec query left); reflexivity.
Qed.

Theorem opposite_arrival_orders_agree_on_every_lookup :
  forall store left right left_value right_value query,
    left <> right ->
    insert_evidence right right_value
      (insert_evidence left left_value store) query =
    insert_evidence left left_value
      (insert_evidence right right_value store) query.
Proof.
  apply distinct_insertions_commute_pointwise.
Qed.

Theorem complete_key_deletion_removes_exact_execution :
  forall store key,
    delete_evidence key store key = None.
Proof.
  intros store key.
  unfold delete_evidence.
  destruct (execution_identity_eq_dec key key); congruence.
Qed.

Theorem complete_key_deletion_preserves_distinct_execution :
  forall store target survivor,
    target <> survivor ->
    delete_evidence target store survivor = store survivor.
Proof.
  intros store target survivor distinct.
  unfold delete_evidence.
  destruct (execution_identity_eq_dec survivor target) as [equal | not_equal].
  - symmetry in equal; contradiction.
  - reflexivity.
Qed.

Theorem deletion_after_distinct_replays_preserves_survivor :
  forall store target survivor target_value survivor_value,
    target <> survivor ->
    delete_evidence target
      (insert_evidence survivor survivor_value
        (insert_evidence target target_value store)) target = None /\
    delete_evidence target
      (insert_evidence survivor survivor_value
        (insert_evidence target target_value store)) survivor = Some survivor_value.
Proof.
  intros store target survivor target_value survivor_value distinct.
  split.
  - apply complete_key_deletion_removes_exact_execution.
  - rewrite complete_key_deletion_preserves_distinct_execution by exact distinct.
    unfold insert_evidence.
    destruct (execution_identity_eq_dec survivor survivor); congruence.
Qed.

Theorem deletion_is_idempotent :
  forall store target query,
    delete_evidence target (delete_evidence target store) query =
    delete_evidence target store query.
Proof.
  intros store target query.
  unfold delete_evidence.
  destruct (execution_identity_eq_dec query target); reflexivity.
Qed.

Theorem deletion_commutes_with_distinct_insertion_pointwise :
  forall store target survivor survivor_value query,
    target <> survivor ->
    delete_evidence target
      (insert_evidence survivor survivor_value store) query =
    insert_evidence survivor survivor_value
      (delete_evidence target store) query.
Proof.
  intros store target survivor survivor_value query distinct.
  unfold delete_evidence, insert_evidence.
  destruct (execution_identity_eq_dec query target) as [query_target | query_not_target].
  - subst query.
    destruct (execution_identity_eq_dec target survivor) as [equal | not_equal].
    + contradiction.
    + destruct (execution_identity_eq_dec target target); congruence.
  - destruct (execution_identity_eq_dec query survivor); reflexivity.
Qed.

Theorem retirement_requires_concrete_latest_witness :
  forall observation,
    retirement_eligible observation = true ->
    retirement_has_latest observation = true.
Proof.
  intros observation eligible.
  unfold retirement_eligible in eligible.
  repeat rewrite Bool.andb_true_iff in eligible.
  tauto.
Qed.

Theorem retirement_requires_every_safety_guard :
  forall observation,
    retirement_eligible observation = true ->
    retirement_finalized observation = true /\
    retirement_beyond_horizon observation = true /\
    retirement_children_known observation = true /\
    retirement_has_child observation = true /\
    retirement_has_latest observation = true /\
    retirement_all_latest_advanced observation = true.
Proof.
  intros observation eligible.
  unfold retirement_eligible in eligible.
  repeat rewrite Bool.andb_true_iff in eligible.
  tauto.
Qed.

Theorem vacuous_latest_guard_is_unsafe :
  exists observation,
    vacuous_latest_retirement_eligible observation = true /\
    retirement_has_latest observation = false /\
    retirement_eligible observation = false.
Proof.
  exists
    {| retirement_finalized := true;
       retirement_beyond_horizon := true;
       retirement_children_known := true;
       retirement_has_child := true;
       retirement_has_latest := false;
       retirement_all_latest_advanced := true |}.
  cbn.
  repeat split; reflexivity.
Qed.

Theorem full_dag_retirement_accepts_every_parent_path :
  forall path,
    retirement_with_path full_dag_recognizes_advancement path = true.
Proof.
  intros path.
  destruct path; reflexivity.
Qed.

Theorem main_spine_only_retirement_is_incomplete :
  retirement_with_path
    full_dag_recognizes_advancement SecondaryParentPath = true /\
  retirement_with_path
    main_spine_recognizes_advancement SecondaryParentPath = false.
Proof.
  split; reflexivity.
Qed.

Print Assumptions complete_key_is_injective.
Print Assumptions distinct_pre_states_have_distinct_keys.
Print Assumptions distinct_post_states_have_distinct_keys.
Print Assumptions distinct_creators_have_distinct_keys.
Print Assumptions distinct_sequences_have_distinct_keys.
Print Assumptions distinct_payloads_have_distinct_keys.
Print Assumptions legacy_key_alias_witness.
Print Assumptions local_replay_publishes_exact_evidence.
Print Assumptions peer_response_cannot_publish_evidence.
Print Assumptions peer_response_cannot_overwrite_local_replay.
Print Assumptions distinct_replays_preserve_both_entries.
Print Assumptions distinct_insertions_commute_pointwise.
Print Assumptions opposite_arrival_orders_agree_on_every_lookup.
Print Assumptions complete_key_deletion_removes_exact_execution.
Print Assumptions complete_key_deletion_preserves_distinct_execution.
Print Assumptions deletion_after_distinct_replays_preserves_survivor.
Print Assumptions deletion_is_idempotent.
Print Assumptions deletion_commutes_with_distinct_insertion_pointwise.
Print Assumptions retirement_requires_concrete_latest_witness.
Print Assumptions retirement_requires_every_safety_guard.
Print Assumptions vacuous_latest_guard_is_unsafe.
Print Assumptions full_dag_retirement_accepts_every_parent_path.
Print Assumptions main_spine_only_retirement_is_incomplete.
