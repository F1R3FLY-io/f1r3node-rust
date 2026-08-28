From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(**
  Universal refinement obligations for bounded kind-1 PREPARE recovery.

  The concrete CRC32C and BLAKE2b functions are deliberately abstract state
  machines here: the important proof obligation is that a fixed-scratch
  chunked fold visits exactly the same bytes, in the same order, as the
  contiguous oracle.  Executable byte differentials bind those abstract
  transitions to the production algorithms after this formal gate passes.

  Likewise, a grammar observation is an optional ranked semantic error.  The
  first pass retains the first such error but drains the authenticated extent;
  record-CRC failure has priority.  This models the existing diagnostic
  contract without materialising the record.
*)

Section ChunkFold.

Context {Byte State : Type}.
Variable step : State -> Byte -> State.

Fixpoint fold_bytes (state : State) (bytes : list Byte) : State :=
  match bytes with
  | [] => state
  | byte :: tail => fold_bytes (step state byte) tail
  end.

Fixpoint fold_chunks (state : State) (chunks : list (list Byte)) : State :=
  match chunks with
  | [] => state
  | chunk :: tail => fold_chunks (fold_bytes state chunk) tail
  end.

Lemma fold_bytes_app :
  forall state left right,
    fold_bytes state (left ++ right) =
    fold_bytes (fold_bytes state left) right.
Proof.
  intros state left; revert state; induction left as [|byte tail IH];
    intros state right; simpl.
  - reflexivity.
  - exact (IH (step state byte) right).
Qed.

Theorem chunk_fold_equals_contiguous_fold :
  forall state chunks,
    fold_chunks state chunks = fold_bytes state (concat chunks).
Proof.
  intros state chunks; revert state; induction chunks as [|chunk tail IH];
    intro state; simpl.
  - reflexivity.
  - rewrite fold_bytes_app. exact (IH (fold_bytes state chunk)).
Qed.

End ChunkFold.

Section FirstSemanticError.

Definition semantic_observation := option nat.

Definition retain_first_error
    (pending observation : semantic_observation) : semantic_observation :=
  match pending with
  | Some rank => Some rank
  | None => observation
  end.

Fixpoint fold_semantic_observations
    (pending : semantic_observation)
    (observations : list semantic_observation) : semantic_observation :=
  match observations with
  | [] => pending
  | observation :: tail =>
      fold_semantic_observations
        (retain_first_error pending observation) tail
  end.

Fixpoint first_semantic_error
    (observations : list semantic_observation) : semantic_observation :=
  match observations with
  | [] => None
  | Some rank :: _ => Some rank
  | None :: tail => first_semantic_error tail
  end.

Lemma retained_error_is_absorbing :
  forall rank observations,
    fold_semantic_observations (Some rank) observations = Some rank.
Proof.
  intros rank observations; induction observations as [|observation tail IH];
    simpl; [reflexivity | exact IH].
Qed.

Theorem observation_fold_retains_exact_first_error :
  forall observations,
    fold_semantic_observations None observations =
    first_semantic_error observations.
Proof.
  intro observations; induction observations as [|observation tail IH]; simpl.
  - reflexivity.
  - destruct observation as [rank|].
    + apply retained_error_is_absorbing.
    + exact IH.
Qed.

Inductive recovery_outcome : Type :=
| StreamAccepted
| StreamCrcRejected
| StreamSemanticRejected (rank : nat).

Definition contiguous_oracle_outcome
    (crc_valid : bool)
    (observations : list semantic_observation) : recovery_outcome :=
  if crc_valid then
    match first_semantic_error observations with
    | Some rank => StreamSemanticRejected rank
    | None => StreamAccepted
    end
  else StreamCrcRejected.

Definition streaming_outcome
    (crc_valid : bool)
    (observations : list semantic_observation) : recovery_outcome :=
  if crc_valid then
    match fold_semantic_observations None observations with
    | Some rank => StreamSemanticRejected rank
    | None => StreamAccepted
    end
  else StreamCrcRejected.

Theorem deferred_semantic_error_refines_contiguous_oracle :
  forall crc_valid observations,
    streaming_outcome crc_valid observations =
    contiguous_oracle_outcome crc_valid observations.
Proof.
  intros crc_valid observations; destruct crc_valid; simpl.
  - rewrite observation_fold_retains_exact_first_error. reflexivity.
  - reflexivity.
Qed.

Theorem crc_failure_dominates_every_semantic_error :
  forall observations,
    streaming_outcome false observations = StreamCrcRejected.
Proof. reflexivity. Qed.

End FirstSemanticError.

Section ExactReplay.

Record layout_observation : Type := LayoutObservation {
  layout_ordinal : nat;
  layout_disposition : nat;
  layout_reference : nat;
  layout_relative_offset : nat;
  layout_final_offset : nat
}.

Definition layout_observation_eqb
    (left right : layout_observation) : bool :=
  Nat.eqb (layout_ordinal left) (layout_ordinal right) &&
  Nat.eqb (layout_disposition left) (layout_disposition right) &&
  Nat.eqb (layout_reference left) (layout_reference right) &&
  Nat.eqb (layout_relative_offset left) (layout_relative_offset right) &&
  Nat.eqb (layout_final_offset left) (layout_final_offset right).

Lemma layout_observation_eqb_true_iff :
  forall left right,
    layout_observation_eqb left right = true <-> left = right.
Proof.
  intros [lo ld lr lro lfo] [ro rd rr rro rfo]; unfold layout_observation_eqb;
    simpl; repeat rewrite andb_true_iff; repeat rewrite Nat.eqb_eq;
    intuition congruence.
Qed.

Fixpoint exact_layout_replay
    (first replay : list layout_observation) : bool :=
  match first, replay with
  | [], [] => true
  | expected :: expected_tail, observed :: observed_tail =>
      layout_observation_eqb expected observed &&
      exact_layout_replay expected_tail observed_tail
  | _, _ => false
  end.

Theorem exact_layout_replay_accepts_iff_identical :
  forall first replay,
    exact_layout_replay first replay = true <-> first = replay.
Proof.
  intro first; induction first as [|expected expected_tail IH];
    intros [|observed observed_tail]; simpl; try (split; discriminate).
  - split; reflexivity.
  - rewrite andb_true_iff, layout_observation_eqb_true_iff, IH.
    split.
    + intros [same_head same_tail]; subst; reflexivity.
    + intro same; inversion same; split; reflexivity.
Qed.

End ExactReplay.

Section ExactSourceReplay.

Fixpoint exact_source_replay (first replay : list nat) : bool :=
  match first, replay with
  | [], [] => true
  | expected :: expected_tail, observed :: observed_tail =>
      Nat.eqb expected observed &&
      exact_source_replay expected_tail observed_tail
  | _, _ => false
  end.

Theorem exact_source_replay_accepts_iff_identical :
  forall first replay,
    exact_source_replay first replay = true <-> first = replay.
Proof.
  intro first; induction first as [|expected expected_tail IH];
    intros [|observed observed_tail]; simpl; try (split; discriminate).
  - split; reflexivity.
  - rewrite andb_true_iff, Nat.eqb_eq, IH.
    split.
    + intros [same_head same_tail]; subst; reflexivity.
    + intro same; inversion same; split; reflexivity.
Qed.

End ExactSourceReplay.

Section ExactBinding.

Record prepare_identity : Type := PrepareIdentity {
  identity_sequence : nat;
  identity_store : nat;
  identity_physical_digest : nat;
  identity_address_len : nat;
  identity_receipt : nat;
  identity_address_offset : nat
}.

Record commit_binding : Type := CommitBinding {
  commit_sequence : nat;
  commit_store : nat;
  commit_physical_digest : nat;
  commit_address_len : nat;
  commit_receipt : nat;
  commit_address_offset : nat
}.

Definition exact_commit_binding
    (prepare : prepare_identity) (commit : commit_binding) : bool :=
  Nat.eqb (identity_sequence prepare) (commit_sequence commit) &&
  Nat.eqb (identity_store prepare) (commit_store commit) &&
  Nat.eqb (identity_physical_digest prepare)
    (commit_physical_digest commit) &&
  Nat.eqb (identity_address_len prepare) (commit_address_len commit) &&
  Nat.eqb (identity_receipt prepare) (commit_receipt commit) &&
  Nat.eqb (identity_address_offset prepare) (commit_address_offset commit).

Definition commit_matches_prepare
    (prepare : prepare_identity) (commit : commit_binding) : Prop :=
  identity_sequence prepare = commit_sequence commit /\
  identity_store prepare = commit_store commit /\
  identity_physical_digest prepare = commit_physical_digest commit /\
  identity_address_len prepare = commit_address_len commit /\
  identity_receipt prepare = commit_receipt commit /\
  identity_address_offset prepare = commit_address_offset commit.

Theorem commit_binding_accepts_exactly_the_prepare_identity :
  forall prepare commit,
    exact_commit_binding prepare commit = true <->
    commit_matches_prepare prepare commit.
Proof.
  intros prepare commit; unfold exact_commit_binding, commit_matches_prepare.
  repeat rewrite andb_true_iff; repeat rewrite Nat.eqb_eq. tauto.
Qed.

End ExactBinding.

Section ResourceAdmission.

Definition resource_admitted (required limit : nat) : bool :=
  Nat.leb required limit.

Theorem exact_resource_boundary_is_admitted :
  forall required, resource_admitted required required = true.
Proof. intro required; apply Nat.leb_refl. Qed.

Theorem required_minus_one_is_rejected :
  forall required,
    0 < required -> resource_admitted required (pred required) = false.
Proof.
  intros required positive; unfold resource_admitted; apply Nat.leb_gt; lia.
Qed.

Record stream_memory : Type := StreamMemory {
  retained_scratch : nat;
  retained_record_buffer : nat;
  retained_key_buffer : nat;
  retained_object_buffer : nat
}.

Definition bounded_stream_memory (scratch_cap : nat)
    (memory : stream_memory) : Prop :=
  retained_scratch memory <= scratch_cap /\
  retained_record_buffer memory = 0 /\
  retained_key_buffer memory = 0 /\
  retained_object_buffer memory = 0.

Definition make_bounded_stream_memory
    (scratch_cap requested : nat) : stream_memory :=
  StreamMemory (Nat.min scratch_cap requested) 0 0 0.

Theorem retained_memory_is_independent_of_record_extent :
  forall scratch_cap requested (record_len key_len object_len : nat),
    bounded_stream_memory scratch_cap
      (make_bounded_stream_memory scratch_cap requested).
Proof.
  intros; unfold bounded_stream_memory, make_bounded_stream_memory; simpl.
  repeat split; try reflexivity. apply Nat.le_min_l.
Qed.

End ResourceAdmission.

Section AuthorityBoundary.

Record validation_state : Type := ValidationState {
  extent_complete : bool;
  crc_validated : bool;
  semantics_validated : bool;
  commit_bound : bool;
  chain_bound : bool;
  source_replay_validated : bool;
  replay_validated : bool;
  source_stable : bool;
  catalog_verified : bool
}.

Definition private_candidate_ready (state : validation_state) : bool :=
  extent_complete state && crc_validated state &&
  semantics_validated state && commit_bound state &&
  chain_bound state && source_replay_validated state &&
  replay_validated state && source_stable state.

Definition validation_accepted (state : validation_state) : bool :=
  private_candidate_ready state && catalog_verified state.

Definition recovered_authority (state : validation_state) : option nat :=
  if validation_accepted state then Some 1 else None.

Theorem authority_exists_iff_every_validation_gate_passes :
  forall state,
    recovered_authority state = Some 1 <->
    extent_complete state = true /\
    crc_validated state = true /\
    semantics_validated state = true /\
    commit_bound state = true /\
    chain_bound state = true /\
    source_replay_validated state = true /\
    replay_validated state = true /\
    source_stable state = true /\
    catalog_verified state = true.
Proof.
  intro state; unfold recovered_authority.
  destruct (validation_accepted state) eqn:accepted.
  - split.
    + intro accepted_authority. unfold validation_accepted in accepted.
      apply andb_true_iff in accepted as [candidate catalog].
      unfold private_candidate_ready in candidate.
      repeat rewrite andb_true_iff in candidate.
      destruct candidate as [[[[[[[extent crc] semantic] commit] chain] source_replay]
        replay] stable]. repeat split; assumption.
    + intro gates. reflexivity.
  - split.
    + discriminate.
    + intros [extent [crc [semantic [commit [chain [source_replay [replay [stable catalog]]]]]]]].
      unfold validation_accepted, private_candidate_ready in accepted.
      rewrite extent, crc, semantic, commit, chain, source_replay, replay, stable, catalog
        in accepted.
      discriminate.
Qed.

Theorem any_failed_gate_exposes_no_authority :
  forall state,
    validation_accepted state = false -> recovered_authority state = None.
Proof.
  intros state rejected; unfold recovered_authority; rewrite rejected; reflexivity.
Qed.

Definition private_index_visible (state : validation_state) : bool :=
  validation_accepted state.

Definition observer_visible (state : validation_state) : bool :=
  validation_accepted state.

Theorem catalog_mismatch_exposes_no_authority_index_or_observer :
  forall state,
    catalog_verified state = false ->
    recovered_authority state = None /\
    private_index_visible state = false /\
    observer_visible state = false.
Proof.
  intros state catalog_mismatch.
  unfold recovered_authority, private_index_visible, observer_visible,
    validation_accepted.
  rewrite catalog_mismatch, andb_false_r. repeat split; reflexivity.
Qed.

Theorem replay_complete_candidate_remains_private_until_catalog_verification :
  forall state,
    private_candidate_ready state = true ->
    catalog_verified state = false ->
    recovered_authority state = None /\
    private_index_visible state = false /\
    observer_visible state = false.
Proof.
  intros state candidate_ready catalog_pending.
  unfold recovered_authority, private_index_visible, observer_visible,
    validation_accepted.
  rewrite candidate_ready, catalog_pending. repeat split; reflexivity.
Qed.

Theorem index_is_not_visible_before_full_authentication :
  forall state,
    private_index_visible state = true ->
    crc_validated state = true /\
    semantics_validated state = true /\
    commit_bound state = true /\
    chain_bound state = true /\
    source_replay_validated state = true /\
    replay_validated state = true /\
    source_stable state = true /\
    catalog_verified state = true.
Proof.
  intros state visible.
  pose proof (proj1 (authority_exists_iff_every_validation_gate_passes state))
    as authority_contract.
  apply authority_contract. unfold private_index_visible in visible.
  unfold recovered_authority. rewrite visible. reflexivity.
Qed.

End AuthorityBoundary.

Print Assumptions chunk_fold_equals_contiguous_fold.
Print Assumptions observation_fold_retains_exact_first_error.
Print Assumptions deferred_semantic_error_refines_contiguous_oracle.
Print Assumptions crc_failure_dominates_every_semantic_error.
Print Assumptions exact_layout_replay_accepts_iff_identical.
Print Assumptions exact_source_replay_accepts_iff_identical.
Print Assumptions commit_binding_accepts_exactly_the_prepare_identity.
Print Assumptions exact_resource_boundary_is_admitted.
Print Assumptions required_minus_one_is_rejected.
Print Assumptions retained_memory_is_independent_of_record_extent.
Print Assumptions authority_exists_iff_every_validation_gate_passes.
Print Assumptions any_failed_gate_exposes_no_authority.
Print Assumptions index_is_not_visible_before_full_authentication.
Print Assumptions catalog_mismatch_exposes_no_authority_index_or_observer.
Print Assumptions replay_complete_candidate_remains_private_until_catalog_verification.
