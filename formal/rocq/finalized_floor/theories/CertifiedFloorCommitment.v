From Stdlib Require Import Bool.Bool Lists.List.
Import ListNotations.

Section CertifiedFloorCommitment.

Context {Floor Effect Digest : Type}.

Variable floor_effects : Floor -> list Effect.
Variable certificate_digest :
  forall predecessor target : Floor, Digest.
Variable causal_certified : Floor -> Prop.
Variable state_certified : Floor -> Prop.

Definition preserves (left right : Floor) : Prop :=
  incl (floor_effects left) (floor_effects right).

Lemma preserves_reflexive :
  forall floor, preserves floor floor.
Proof.
  intros floor effect present.
  exact present.
Qed.

Lemma preserves_transitive :
  forall left middle right,
    preserves left middle ->
    preserves middle right ->
    preserves left right.
Proof.
  intros left middle right left_middle middle_right effect present.
  apply middle_right.
  apply left_middle.
  exact present.
Qed.

Record finalization_certificate : Type := {
  certificate_predecessor : Floor;
  certificate_target : Floor
}.

Definition valid_certificate (certificate : finalization_certificate) : Prop :=
  causal_certified (certificate_target certificate) /\
  state_certified (certificate_target certificate) /\
  preserves
    (certificate_predecessor certificate)
    (certificate_target certificate).

Definition digest_of (certificate : finalization_certificate) : Digest :=
  certificate_digest
    (certificate_predecessor certificate)
    (certificate_target certificate).

Record floor_commitment : Type := {
  committed_floor : Floor;
  committed_certificate_digest : Digest;
  committed_authority_context_digest : Digest
}.

Definition valid_commitment
  (commitment : floor_commitment)
  (certificate : finalization_certificate)
  : Prop :=
  committed_floor commitment = certificate_target certificate /\
  committed_certificate_digest commitment = digest_of certificate /\
  valid_certificate certificate.

Record candidate : Type := {
  candidate_captured_floor : Floor;
  candidate_structural_parent_floor : Floor;
  candidate_parent_floors : list Floor;
  candidate_commitment : floor_commitment;
  candidate_authority_context_digest : Digest;
  candidate_effects : list Effect
}.

Definition candidate_preserves
  (floor : Floor)
  (block : candidate)
  : Prop :=
  incl (floor_effects floor) (candidate_effects block).

Definition all_parent_floors_preserved (block : candidate) : Prop :=
  forall parent_floor,
    In parent_floor (candidate_parent_floors block) ->
    preserves parent_floor (committed_floor (candidate_commitment block)).

Definition commitment_binds_candidate_context (block : candidate) : Prop :=
  committed_authority_context_digest (candidate_commitment block) =
  candidate_authority_context_digest block.

Definition candidate_specific_admission (block : candidate) : Prop :=
  preserves
    (candidate_captured_floor block)
    (committed_floor (candidate_commitment block)) /\
  all_parent_floors_preserved block /\
  candidate_preserves
    (committed_floor (candidate_commitment block))
    block /\
  commitment_binds_candidate_context block.

Definition certificate_chain_gate
  (cache_hit : bool)
  (certificate : finalization_certificate)
  : Prop :=
  if cache_hit then True else valid_certificate certificate.

Definition candidate_admissible
  (cache_hit : bool)
  (block : candidate)
  (certificate : finalization_certificate)
  : Prop :=
  valid_commitment (candidate_commitment block) certificate /\
  certificate_chain_gate cache_hit certificate /\
  candidate_specific_admission block.

Definition valid_candidate
  (block : candidate)
  (certificate : finalization_certificate)
  : Prop :=
  valid_commitment (candidate_commitment block) certificate /\
  candidate_specific_admission block.

Theorem valid_candidate_preserves_captured_floor :
  forall block certificate,
    valid_candidate block certificate ->
    candidate_preserves (candidate_captured_floor block) block.
Proof.
  intros block certificate [_ [captured_to_committed [_ [committed_to_block _]]]].
  intros effect present.
  apply committed_to_block.
  apply captured_to_committed.
  exact present.
Qed.

Theorem stale_structural_floor_does_not_regress_committed_floor :
  forall block certificate stale_floor,
    valid_candidate block certificate ->
    candidate_structural_parent_floor block = stale_floor ->
    preserves
      (candidate_captured_floor block)
      (committed_floor (candidate_commitment block)).
Proof.
  intros block certificate stale_floor [_ [preserved _]] _.
  exact preserved.
Qed.

Theorem valid_candidate_preserves_every_parent_floor :
  forall block certificate parent_floor,
    valid_candidate block certificate ->
    In parent_floor (candidate_parent_floors block) ->
    preserves parent_floor (committed_floor (candidate_commitment block)).
Proof.
  intros block certificate parent_floor [_ [_ [parents _]]] present.
  apply parents.
  exact present.
Qed.

Theorem valid_candidate_binds_candidate_authority_context :
  forall block certificate,
    valid_candidate block certificate ->
    committed_authority_context_digest (candidate_commitment block) =
    candidate_authority_context_digest block.
Proof.
  intros block certificate [_ [_ [_ [_ bound]]]].
  exact bound.
Qed.

Theorem cached_admission_preserves_every_parent_floor :
  forall block certificate parent_floor,
    candidate_admissible true block certificate ->
    In parent_floor (candidate_parent_floors block) ->
    preserves parent_floor (committed_floor (candidate_commitment block)).
Proof.
  intros block certificate parent_floor [_ [_ [_ [parents _]]]] present.
  apply parents.
  exact present.
Qed.

Theorem certificate_cache_transparent_for_valid_certificate :
  forall cache_hit block certificate,
    valid_certificate certificate ->
    (candidate_admissible cache_hit block certificate <->
     candidate_admissible false block certificate).
Proof.
  intros cache_hit block certificate certificate_valid.
  unfold candidate_admissible, certificate_chain_gate.
  destruct cache_hit; simpl.
  - split.
    + intros [commitment_valid [_ candidate_valid]].
      exact (conj commitment_valid (conj certificate_valid candidate_valid)).
    + intros [commitment_valid [_ candidate_valid]].
      exact (conj commitment_valid (conj I candidate_valid)).
  - tauto.
Qed.

Theorem candidate_admission_independent_of_receiver_local_floor :
  forall cache_hit block certificate (receiver_left receiver_right : Floor),
    candidate_admissible cache_hit block certificate <->
    candidate_admissible cache_hit block certificate.
Proof.
  intros cache_hit block certificate receiver_left receiver_right.
  reflexivity.
Qed.

Theorem commitment_binds_exact_certificate_target :
  forall commitment certificate,
    valid_commitment commitment certificate ->
    committed_floor commitment = certificate_target certificate /\
    committed_certificate_digest commitment = digest_of certificate /\
    causal_certified (certificate_target certificate) /\
    state_certified (certificate_target certificate) /\
    preserves
      (certificate_predecessor certificate)
      (certificate_target certificate).
Proof.
  intros commitment certificate [target [digest [causal [state lineage]]]].
  repeat split; assumption.
Qed.

Theorem distinct_certificate_substitution_requires_digest_collision :
  forall commitment expected substituted,
    valid_commitment commitment expected ->
    committed_certificate_digest commitment = digest_of substituted ->
    expected <> substituted ->
    digest_of expected = digest_of substituted.
Proof.
  intros commitment expected substituted [_ [bound _]] substituted_bound _.
  rewrite <- bound.
  exact substituted_bound.
Qed.

Fixpoint valid_certificate_chain
  (anchor : Floor)
  (certificates : list finalization_certificate)
  : Prop :=
  match certificates with
  | [] => True
  | certificate :: tail =>
      certificate_predecessor certificate = anchor /\
      valid_certificate certificate /\
      valid_certificate_chain (certificate_target certificate) tail
  end.

Fixpoint certificate_chain_target
  (anchor : Floor)
  (certificates : list finalization_certificate)
  : Floor :=
  match certificates with
  | [] => anchor
  | certificate :: tail =>
      certificate_chain_target (certificate_target certificate) tail
  end.

Theorem valid_certificate_chain_preserves_anchor :
  forall certificates anchor,
    valid_certificate_chain anchor certificates ->
    preserves anchor (certificate_chain_target anchor certificates).
Proof.
  induction certificates as [| certificate tail induction]; intros anchor valid.
  - simpl.
    apply preserves_reflexive.
  - simpl in valid |- *.
    destruct valid as [predecessor [certificate_valid tail_valid]].
    destruct certificate_valid as [_ [_ certificate_lineage]].
    subst anchor.
    eapply preserves_transitive.
    + exact certificate_lineage.
    + apply induction.
      exact tail_valid.
Qed.

Inductive dependency_admission : Type :=
| Buffered
| Accepted
| Rejected.

Definition admit_dependency
  (certificate_known certificate_valid : bool)
  : dependency_admission :=
  if certificate_known
  then if certificate_valid then Accepted else Rejected
  else Buffered.

Theorem missing_certificate_never_accepts :
  forall valid,
    admit_dependency false valid = Buffered.
Proof.
  intros valid.
  reflexivity.
Qed.

Theorem fetched_valid_certificate_accepts :
  admit_dependency true true = Accepted.
Proof.
  reflexivity.
Qed.

Theorem fetched_invalid_certificate_rejects :
  admit_dependency true false = Rejected.
Proof.
  reflexivity.
Qed.

Theorem certified_floor_commitment_contract :
  (forall block certificate,
    valid_candidate block certificate ->
    candidate_preserves (candidate_captured_floor block) block)
  /\
  (forall certificates anchor,
    valid_certificate_chain anchor certificates ->
    preserves anchor (certificate_chain_target anchor certificates))
  /\
  (forall valid, admit_dependency false valid = Buffered)
  /\
  admit_dependency true true = Accepted
  /\
  admit_dependency true false = Rejected
  /\
  (forall block certificate parent_floor,
    candidate_admissible true block certificate ->
    In parent_floor (candidate_parent_floors block) ->
    preserves parent_floor (committed_floor (candidate_commitment block)))
  /\
  (forall cache_hit block certificate,
    valid_certificate certificate ->
    (candidate_admissible cache_hit block certificate <->
     candidate_admissible false block certificate)).
Proof.
  split.
  - exact valid_candidate_preserves_captured_floor.
  - split.
    + exact valid_certificate_chain_preserves_anchor.
    + split.
      * exact missing_certificate_never_accepts.
      * split.
        -- exact fetched_valid_certificate_accepts.
        -- split.
           ++ exact fetched_invalid_certificate_rejects.
           ++ split.
              ** exact cached_admission_preserves_every_parent_floor.
              ** exact certificate_cache_transparent_for_valid_certificate.
Qed.

End CertifiedFloorCommitment.

Print Assumptions certified_floor_commitment_contract.
