From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.

Import ListNotations.

Set Implicit Arguments.

Section ProtocolV5DependencyReadiness.

Variable Hash : Type.
Variable hash_eq_dec : forall left right : Hash, {left = right} + {left <> right}.

Inductive ProtocolDependencyEvidence : Type :=
| ProtocolLegacyUnary : Hash -> ProtocolDependencyEvidence
| ProtocolObjectivePair : Hash -> Hash -> ProtocolDependencyEvidence.

Record ProtocolDependencyOrigins : Type := {
  protocol_parents : list Hash;
  protocol_justifications : list Hash;
  protocol_slash_evidence : list ProtocolDependencyEvidence;
  protocol_header_evidence : list (Hash * Hash)
}.

Definition protocol_evidence_dependencies
    (evidence : ProtocolDependencyEvidence) : list Hash :=
  match evidence with
  | ProtocolLegacyUnary block_hash => [block_hash]
  | ProtocolObjectivePair first second => [first; second]
  end.

Definition protocol_header_dependencies (pair : Hash * Hash) : list Hash :=
  let '(first, second) := pair in [first; second].

Definition protocol_raw_dependencies
    (origins : ProtocolDependencyOrigins) : list Hash :=
  protocol_parents origins
  ++ protocol_justifications origins
  ++ flat_map protocol_evidence_dependencies (protocol_slash_evidence origins)
  ++ flat_map protocol_header_dependencies (protocol_header_evidence origins).

Definition protocol_dependencies
    (origins : ProtocolDependencyOrigins) : list Hash :=
  nodup hash_eq_dec (protocol_raw_dependencies origins).

Definition protocol_all_admitted
    (metadata : list Hash)
    (origins : ProtocolDependencyOrigins) : Prop :=
  forall block_hash,
    In block_hash (protocol_dependencies origins) ->
    In block_hash metadata.

Definition protocol_dependency_present
    (metadata : list Hash)
    (block_hash : Hash) : bool :=
  if in_dec hash_eq_dec block_hash metadata then true else false.

Definition protocol_readyb
    (metadata : list Hash)
    (origins : ProtocolDependencyOrigins) : bool :=
  forallb (protocol_dependency_present metadata) (protocol_dependencies origins).

Definition protocol_direct_ready
    (metadata invalid_index tracker : list Hash)
    (origins : ProtocolDependencyOrigins) : Prop :=
  protocol_all_admitted metadata origins.

Definition protocol_buffer_ready
    (metadata invalid_index tracker : list Hash)
    (origins : ProtocolDependencyOrigins) : Prop :=
  protocol_all_admitted metadata origins.

Definition protocol_same_dependency_set
    (left right : ProtocolDependencyOrigins) : Prop :=
  forall block_hash,
    In block_hash (protocol_dependencies left) <->
    In block_hash (protocol_dependencies right).

Lemma protocol_dependency_present_spec :
  forall metadata block_hash,
    protocol_dependency_present metadata block_hash = true <->
    In block_hash metadata.
Proof.
  intros metadata block_hash.
  unfold protocol_dependency_present.
  destruct (in_dec hash_eq_dec block_hash metadata); split; intros; auto.
  discriminate.
Qed.

Theorem protocol_readyb_spec :
  forall metadata origins,
    protocol_readyb metadata origins = true <->
    protocol_all_admitted metadata origins.
Proof.
  intros metadata origins.
  unfold protocol_readyb, protocol_all_admitted.
  rewrite forallb_forall.
  split.
  - intros ready block_hash dependency.
    apply protocol_dependency_present_spec.
    apply ready.
    exact dependency.
  - intros admitted block_hash dependency.
    apply protocol_dependency_present_spec.
    apply admitted.
    exact dependency.
Qed.

Theorem protocol_dependency_membership_exact :
  forall origins block_hash,
    In block_hash (protocol_dependencies origins) <->
    In block_hash (protocol_raw_dependencies origins).
Proof.
  intros origins block_hash.
  unfold protocol_dependencies.
  apply nodup_In.
Qed.

Lemma protocol_parent_is_raw_dependency :
  forall origins block_hash,
    In block_hash (protocol_parents origins) ->
    In block_hash (protocol_raw_dependencies origins).
Proof.
  intros origins block_hash dependency.
  unfold protocol_raw_dependencies.
  apply in_or_app.
  left.
  exact dependency.
Qed.

Lemma protocol_justification_is_raw_dependency :
  forall origins block_hash,
    In block_hash (protocol_justifications origins) ->
    In block_hash (protocol_raw_dependencies origins).
Proof.
  intros origins block_hash dependency.
  unfold protocol_raw_dependencies.
  apply in_or_app.
  right.
  apply in_or_app.
  left.
  exact dependency.
Qed.

Lemma protocol_slash_evidence_hash_is_raw_dependency :
  forall origins evidence block_hash,
    In evidence (protocol_slash_evidence origins) ->
    In block_hash (protocol_evidence_dependencies evidence) ->
    In block_hash (protocol_raw_dependencies origins).
Proof.
  intros origins evidence block_hash evidence_member hash_member.
  unfold protocol_raw_dependencies.
  apply in_or_app.
  right.
  apply in_or_app.
  right.
  apply in_or_app.
  left.
  apply in_flat_map.
  exists evidence.
  split; assumption.
Qed.

Lemma protocol_header_hash_is_raw_dependency :
  forall origins pair block_hash,
    In pair (protocol_header_evidence origins) ->
    In block_hash (protocol_header_dependencies pair) ->
    In block_hash (protocol_raw_dependencies origins).
Proof.
  intros origins pair block_hash pair_member hash_member.
  unfold protocol_raw_dependencies.
  apply in_or_app.
  right.
  apply in_or_app.
  right.
  apply in_or_app.
  right.
  apply in_flat_map.
  exists pair.
  split; assumption.
Qed.

Theorem protocol_parent_is_dependency :
  forall origins block_hash,
    In block_hash (protocol_parents origins) ->
    In block_hash (protocol_dependencies origins).
Proof.
  intros origins block_hash dependency.
  apply protocol_dependency_membership_exact.
  apply protocol_parent_is_raw_dependency.
  exact dependency.
Qed.

Theorem protocol_justification_is_dependency :
  forall origins block_hash,
    In block_hash (protocol_justifications origins) ->
    In block_hash (protocol_dependencies origins).
Proof.
  intros origins block_hash dependency.
  apply protocol_dependency_membership_exact.
  apply protocol_justification_is_raw_dependency.
  exact dependency.
Qed.

Theorem protocol_legacy_unary_is_dependency :
  forall origins block_hash,
    In (ProtocolLegacyUnary block_hash) (protocol_slash_evidence origins) ->
    In block_hash (protocol_dependencies origins).
Proof.
  intros origins block_hash evidence_member.
  apply protocol_dependency_membership_exact.
  eapply protocol_slash_evidence_hash_is_raw_dependency.
  - exact evidence_member.
  - simpl.
    auto.
Qed.

Theorem protocol_objective_pair_is_complete :
  forall origins first second,
    In (ProtocolObjectivePair first second) (protocol_slash_evidence origins) ->
    In first (protocol_dependencies origins)
    /\ In second (protocol_dependencies origins).
Proof.
  intros origins first second evidence_member.
  split.
  - apply protocol_dependency_membership_exact.
    eapply protocol_slash_evidence_hash_is_raw_dependency.
    + exact evidence_member.
    + simpl.
      auto.
  - apply protocol_dependency_membership_exact.
    eapply protocol_slash_evidence_hash_is_raw_dependency.
    + exact evidence_member.
    + simpl.
      auto.
Qed.

Theorem protocol_header_pair_is_complete :
  forall origins first second,
    In (first, second) (protocol_header_evidence origins) ->
    In first (protocol_dependencies origins)
    /\ In second (protocol_dependencies origins).
Proof.
  intros origins first second pair_member.
  split.
  - apply protocol_dependency_membership_exact.
    eapply protocol_header_hash_is_raw_dependency.
    + exact pair_member.
    + simpl.
      auto.
  - apply protocol_dependency_membership_exact.
    eapply protocol_header_hash_is_raw_dependency.
    + exact pair_member.
    + simpl.
      auto.
Qed.

Theorem protocol_ready_objective_pair_admitted :
  forall metadata origins first second,
    protocol_all_admitted metadata origins ->
    In (ProtocolObjectivePair first second) (protocol_slash_evidence origins) ->
    In first metadata /\ In second metadata.
Proof.
  intros metadata origins first second admitted evidence_member.
  destruct (protocol_objective_pair_is_complete
              origins first second evidence_member)
    as [first_dep second_dep].
  split; apply admitted; assumption.
Qed.

Theorem protocol_ready_header_pair_admitted :
  forall metadata origins first second,
    protocol_all_admitted metadata origins ->
    In (first, second) (protocol_header_evidence origins) ->
    In first metadata /\ In second metadata.
Proof.
  intros metadata origins first second admitted pair_member.
  destruct (protocol_header_pair_is_complete
              origins first second pair_member)
    as [first_dep second_dep].
  split; apply admitted; assumption.
Qed.

Theorem protocol_invalid_index_noninterference :
  forall metadata invalid_left invalid_right tracker origins,
    protocol_direct_ready metadata invalid_left tracker origins <->
    protocol_direct_ready metadata invalid_right tracker origins.
Proof.
  intros.
  unfold protocol_direct_ready.
  tauto.
Qed.

Theorem protocol_tracker_noninterference :
  forall metadata invalid_index tracker_left tracker_right origins,
    protocol_direct_ready metadata invalid_index tracker_left origins <->
    protocol_direct_ready metadata invalid_index tracker_right origins.
Proof.
  intros.
  unfold protocol_direct_ready.
  tauto.
Qed.

Theorem protocol_direct_buffer_readiness_equal :
  forall metadata invalid_index tracker origins,
    protocol_direct_ready metadata invalid_index tracker origins <->
    protocol_buffer_ready metadata invalid_index tracker origins.
Proof.
  intros.
  unfold protocol_direct_ready, protocol_buffer_ready.
  tauto.
Qed.

Theorem protocol_dependency_permutation_invariant :
  forall left right,
    Permutation (protocol_raw_dependencies left)
                (protocol_raw_dependencies right) ->
    protocol_same_dependency_set left right.
Proof.
  intros left right permutation block_hash.
  repeat rewrite protocol_dependency_membership_exact.
  split.
  - intro dependency.
    eapply Permutation_in.
    + exact permutation.
    + exact dependency.
  - intro dependency.
    eapply Permutation_in.
    + apply Permutation_sym.
      exact permutation.
    + exact dependency.
Qed.

End ProtocolV5DependencyReadiness.
