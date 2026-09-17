From Stdlib Require Import Lists.List.
Import ListNotations.

From FinalizedFloor Require Import StateLineageFinality.

Section ParallelValidatorConsensus.

Context {Node Block Root Effect : Type}.
Variable node_eq_dec : forall left right : Node, {left = right} + {left <> right}.
Variable block_root : Block -> Root.
Variable block_effects : Block -> list Effect.
Variable certified : Block -> Prop.
Variable state_certified : Block -> Prop.

Definition effect_ancestor (ancestor descendant : Block) : Prop :=
  incl (block_effects ancestor) (block_effects descendant).

Lemma effect_ancestor_reflexive :
  forall block, effect_ancestor block block.
Proof.
  intros block effect present.
  exact present.
Qed.

Lemma effect_ancestor_transitive :
  forall left middle right,
    effect_ancestor left middle ->
    effect_ancestor middle right ->
    effect_ancestor left right.
Proof.
  intros left middle right left_middle middle_right effect present.
  apply middle_right.
  apply left_middle.
  exact present.
Qed.

Record local_validation := {
  validation_captured_floor : Block;
  validation_captured_root : Root;
  validation_candidate : Block;
  validation_replay_root : Root;
  validation_replay_effects : list Effect;
  validation_accepted : bool;
  validation_support_emitted : bool
}.

Definition local_validation_sound
  (current candidate : Block)
  (validation : local_validation)
  : Prop :=
  validation_captured_floor validation = current /\
  validation_captured_root validation = block_root current /\
  validation_candidate validation = candidate /\
  validation_replay_root validation = block_root candidate /\
  validation_replay_effects validation = block_effects candidate /\
  incl (block_effects current) (validation_replay_effects validation) /\
  validation_accepted validation = true /\
  (validation_support_emitted validation = true ->
    validation_accepted validation = true).

Theorem local_support_requires_accepted_replay :
  forall current candidate validation,
    local_validation_sound current candidate validation ->
    validation_support_emitted validation = true ->
    validation_accepted validation = true.
Proof.
  intros current candidate validation sound support.
  destruct sound as [_ [_ [_ [_ [_ [_ [_ support_sound]]]]]]].
  apply support_sound.
  exact support.
Qed.

Theorem sound_local_validation_preserves_captured_state :
  forall current candidate validation,
    local_validation_sound current candidate validation ->
    effect_ancestor current candidate.
Proof.
  intros current candidate validation sound.
  destruct sound as [_ [_ [_ [_ [replay_exact [preserves _]]]]]].
  unfold effect_ancestor.
  rewrite <- replay_exact.
  exact preserves.
Qed.

Theorem sound_certified_validation_is_eligible :
  forall current candidate validation,
    local_validation_sound current candidate validation ->
    certified candidate ->
    state_certified candidate ->
    lfb_eligible certified state_certified effect_ancestor current candidate.
Proof.
  intros current candidate validation sound causal_certificate state_certificate.
  repeat split.
  - exact causal_certificate.
  - exact state_certificate.
  - apply sound_local_validation_preserves_captured_state with validation.
    exact sound.
Qed.

Record validator_state := {
  validator_finality : @finality_state Block;
  validator_published_root : Root;
  validator_published_effects : list Effect;
  validator_recorded_root : Root -> Prop
}.

Definition validator_consistent (state : validator_state) : Prop :=
  validator_published_root state =
    block_root (current_lfb (validator_finality state)) /\
  validator_published_effects state =
    block_effects (current_lfb (validator_finality state)) /\
  lineage_invariant effect_ancestor (validator_finality state) /\
  validator_recorded_root state (validator_published_root state).

Definition record_validator_root
  (state : validator_state)
  (root : Root)
  : validator_state :=
  {|
    validator_finality := validator_finality state;
    validator_published_root := validator_published_root state;
    validator_published_effects := validator_published_effects state;
    validator_recorded_root :=
      fun candidate => candidate = root \/ validator_recorded_root state candidate
  |}.

Theorem replay_records_its_local_root :
  forall state root,
    validator_recorded_root (record_validator_root state root) root.
Proof.
  intros state root.
  simpl.
  left.
  reflexivity.
Qed.

Theorem replay_root_recording_preserves_consistency :
  forall state root,
    validator_consistent state ->
    validator_consistent (record_validator_root state root).
Proof.
  intros state root consistent.
  destruct consistent as [published_root [published_effects [lineage recorded]]].
  unfold validator_consistent, record_validator_root.
  simpl.
  repeat split; try assumption.
  right.
  exact recorded.
Qed.

Definition promote_validator_state
  (state : validator_state)
  (candidate : Block)
  : validator_state :=
  {|
    validator_finality := promote (validator_finality state) candidate;
    validator_published_root := block_root candidate;
    validator_published_effects := block_effects candidate;
    validator_recorded_root := validator_recorded_root state
  |}.

Theorem eligible_validator_promotion_is_atomic_and_lineage_preserving :
  forall state candidate,
    validator_consistent state ->
    lfb_eligible certified state_certified effect_ancestor
      (current_lfb (validator_finality state)) candidate ->
    validator_recorded_root state (block_root candidate) ->
    validator_consistent (promote_validator_state state candidate).
Proof.
  intros state candidate consistent eligible recorded_candidate.
  destruct consistent as [root_exact [effects_exact [lineage recorded_current]]].
  unfold validator_consistent, promote_validator_state.
  simpl.
  split; [reflexivity |].
  split; [reflexivity |].
  split.
  - eapply eligible_promotion_preserves_lineage.
    + apply effect_ancestor_reflexive.
    + apply effect_ancestor_transitive.
    + exact lineage.
    + exact eligible.
  - exact recorded_candidate.
Qed.

Theorem promotion_retains_every_recorded_root :
  forall state candidate root,
    validator_recorded_root state root ->
    validator_recorded_root (promote_validator_state state candidate) root.
Proof.
  intros state candidate root recorded.
  exact recorded.
Qed.

Definition restart_validator_state (state : validator_state) : validator_state := state.

Theorem restart_preserves_consistency_and_recorded_roots :
  forall state,
    validator_consistent state ->
    validator_consistent (restart_validator_state state) /\
    (forall root,
      validator_recorded_root (restart_validator_state state) root <->
      validator_recorded_root state root).
Proof.
  intros state consistent.
  split.
  - exact consistent.
  - intros root.
    reflexivity.
Qed.

Definition replace_validator
  (world : Node -> validator_state)
  (target : Node)
  (replacement : validator_state)
  : Node -> validator_state :=
  fun observer =>
    if node_eq_dec observer target then replacement else world observer.

Definition promote_validator
  (world : Node -> validator_state)
  (target : Node)
  (candidate : Block)
  : Node -> validator_state :=
  replace_validator world target
    (promote_validator_state (world target) candidate).

Lemma replace_validator_at_target :
  forall world target replacement,
    replace_validator world target replacement target = replacement.
Proof.
  intros world target replacement.
  unfold replace_validator.
  destruct (node_eq_dec target target); congruence.
Qed.

Lemma replace_validator_frames_other :
  forall world target replacement observer,
    observer <> target ->
    replace_validator world target replacement observer = world observer.
Proof.
  intros world target replacement observer distinct.
  unfold replace_validator.
  destruct (node_eq_dec observer target); congruence.
Qed.

Theorem validator_promotion_frames_every_other_node :
  forall world target candidate observer,
    observer <> target ->
    promote_validator world target candidate observer = world observer.
Proof.
  intros world target candidate observer distinct.
  unfold promote_validator.
  apply replace_validator_frames_other.
  exact distinct.
Qed.

Theorem validator_promotion_preserves_global_consistency :
  forall world target candidate,
    (forall node, validator_consistent (world node)) ->
    lfb_eligible certified state_certified effect_ancestor
      (current_lfb (validator_finality (world target))) candidate ->
    validator_recorded_root (world target) (block_root candidate) ->
    forall node,
      validator_consistent (promote_validator world target candidate node).
Proof.
  intros world target candidate all_consistent eligible recorded_candidate node.
  destruct (node_eq_dec node target) as [same | distinct].
  - subst node.
    unfold promote_validator.
    rewrite replace_validator_at_target.
    apply eligible_validator_promotion_is_atomic_and_lineage_preserving.
    + apply all_consistent.
    + exact eligible.
    + exact recorded_candidate.
  - rewrite validator_promotion_frames_every_other_node by exact distinct.
    apply all_consistent.
Qed.

Theorem distinct_validator_promotions_commute_pointwise :
  forall world left right left_candidate right_candidate observer,
    left <> right ->
    promote_validator
      (promote_validator world left left_candidate)
      right right_candidate observer =
    promote_validator
      (promote_validator world right right_candidate)
      left left_candidate observer.
Proof.
  intros world left right left_candidate right_candidate observer distinct.
  unfold promote_validator, replace_validator.
  destruct (node_eq_dec observer right) as [observer_right | observer_not_right].
  - subst observer.
    destruct (node_eq_dec right left) as [same | right_not_left].
    + exfalso.
      apply distinct.
      symmetry.
      exact same.
    + destruct (node_eq_dec right right); try contradiction.
      reflexivity.
  - destruct (node_eq_dec observer left) as [observer_left | observer_not_left].
    + subst observer.
      destruct (node_eq_dec left right) as [same | left_not_right].
      * contradiction.
      * destruct (node_eq_dec left left); try contradiction.
        reflexivity.
    + destruct (node_eq_dec observer right); try contradiction.
      destruct (node_eq_dec observer left); try contradiction.
      reflexivity.
Qed.

Theorem validators_promoting_the_same_candidate_publish_identical_state :
  forall world left right candidate,
    validator_published_root
      (promote_validator world left candidate left) =
    validator_published_root
      (promote_validator world right candidate right) /\
    validator_published_effects
      (promote_validator world left candidate left) =
    validator_published_effects
      (promote_validator world right candidate right).
Proof.
  intros world left right candidate.
  unfold promote_validator.
  repeat rewrite replace_validator_at_target.
  split; reflexivity.
Qed.

Definition parallel_validator_contract : Prop :=
  (forall current candidate validation,
    local_validation_sound current candidate validation ->
    certified candidate ->
    state_certified candidate ->
    lfb_eligible certified state_certified effect_ancestor current candidate) /\
  (forall world target candidate,
    (forall node, validator_consistent (world node)) ->
    lfb_eligible certified state_certified effect_ancestor
      (current_lfb (validator_finality (world target))) candidate ->
    validator_recorded_root (world target) (block_root candidate) ->
    forall node,
      validator_consistent (promote_validator world target candidate node)) /\
  (forall world left right left_candidate right_candidate observer,
    left <> right ->
    promote_validator
      (promote_validator world left left_candidate)
      right right_candidate observer =
    promote_validator
      (promote_validator world right right_candidate)
      left left_candidate observer) /\
  (forall world left right candidate,
    validator_published_root
      (promote_validator world left candidate left) =
    validator_published_root
      (promote_validator world right candidate right) /\
    validator_published_effects
      (promote_validator world left candidate left) =
      validator_published_effects
        (promote_validator world right candidate right)) /\
  (forall state candidate root,
    validator_recorded_root state root ->
    validator_recorded_root (promote_validator_state state candidate) root) /\
  (forall state,
    validator_consistent state ->
    validator_consistent (restart_validator_state state) /\
    (forall root,
      validator_recorded_root (restart_validator_state state) root <->
      validator_recorded_root state root)).

Theorem parallel_validator_consensus_correct : parallel_validator_contract.
Proof.
  unfold parallel_validator_contract.
  split.
  - exact sound_certified_validation_is_eligible.
  - split.
    + exact validator_promotion_preserves_global_consistency.
    + split.
      * exact distinct_validator_promotions_commute_pointwise.
      * split.
        -- exact validators_promoting_the_same_candidate_publish_identical_state.
        -- split.
          ++ exact promotion_retains_every_recorded_root.
          ++ exact restart_preserves_consistency_and_recorded_roots.
Qed.

End ParallelValidatorConsensus.

Print Assumptions local_support_requires_accepted_replay.
Print Assumptions sound_certified_validation_is_eligible.
Print Assumptions replay_root_recording_preserves_consistency.
Print Assumptions eligible_validator_promotion_is_atomic_and_lineage_preserving.
Print Assumptions promotion_retains_every_recorded_root.
Print Assumptions restart_preserves_consistency_and_recorded_roots.
Print Assumptions distinct_validator_promotions_commute_pointwise.
Print Assumptions parallel_validator_consensus_correct.
