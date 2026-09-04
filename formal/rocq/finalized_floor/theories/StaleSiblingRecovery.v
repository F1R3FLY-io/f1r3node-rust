From Stdlib Require Import List Bool.Bool.
Import ListNotations.

Inductive recovery_effect : Type :=
| StaleA
| FloorB
| FreshWork.

Inductive block_source : Type :=
| SourceA
| SourceB.

Record exact_rejection : Type := {
  rejected_effect : recovery_effect;
  rejected_source : block_source
}.

Record stale_sibling_state : Type := {
  causal_sources : list block_source;
  floor_effects : list recovery_effect;
  exact_rejections : list exact_rejection;
  rejected_buffer : list recovery_effect;
  selected_recovery : list recovery_effect;
  committed_effects : list recovery_effect
}.

Definition accepted_siblings : stale_sibling_state :=
  {| causal_sources := [SourceA; SourceB];
     floor_effects := [];
     exact_rejections := [];
     rejected_buffer := [];
     selected_recovery := [];
     committed_effects := [] |}.

Definition finalize_majority_b (state : stale_sibling_state)
  : stale_sibling_state :=
  {| causal_sources := causal_sources state;
     floor_effects := [FloorB];
     exact_rejections := exact_rejections state;
     rejected_buffer := rejected_buffer state;
     selected_recovery := selected_recovery state;
     committed_effects := [FloorB] |}.

Definition settle_exact_frontier (_ : stale_sibling_state)
  : stale_sibling_state :=
  {| causal_sources := [SourceB];
     floor_effects := [FloorB];
     exact_rejections :=
       [{| rejected_effect := StaleA; rejected_source := SourceA |}];
     rejected_buffer := [StaleA];
     selected_recovery := [];
     committed_effects := [FloorB] |}.

Definition recovery_effect_eqb (left right : recovery_effect) : bool :=
  match left, right with
  | StaleA, StaleA | FloorB, FloorB | FreshWork, FreshWork => true
  | _, _ => false
  end.

Definition source_eqb (left right : block_source) : bool :=
  match left, right with
  | SourceA, SourceA | SourceB, SourceB => true
  | _, _ => false
  end.

Definition has_exact_a_tombstone (state : stale_sibling_state) : bool :=
  existsb
    (fun rejection =>
      recovery_effect_eqb (rejected_effect rejection) StaleA &&
      source_eqb (rejected_source rejection) SourceA)
    (exact_rejections state).

Definition has_buffered_a (state : stale_sibling_state) : bool :=
  existsb
    (fun effect => recovery_effect_eqb effect StaleA)
    (rejected_buffer state).

Definition stale_recovery_authorized (state : stale_sibling_state) : bool :=
  has_exact_a_tombstone state && has_buffered_a state.

Definition publish_elected_recovery (state : stale_sibling_state)
  : option stale_sibling_state :=
  if stale_recovery_authorized state then
    Some
      {| causal_sources := [SourceB];
         floor_effects := [FloorB];
         exact_rejections := exact_rejections state;
         rejected_buffer := rejected_buffer state;
         selected_recovery := [StaleA; FreshWork];
         committed_effects := [StaleA; FloorB; FreshWork] |}
  else None.

Theorem finalized_b_retains_accepted_stale_causal_source :
  In SourceA (causal_sources (finalize_majority_b accepted_siblings)).
Proof.
  simpl; auto.
Qed.

Theorem retry_requires_committed_exact_rejection :
  publish_elected_recovery accepted_siblings = None /\
  publish_elected_recovery (finalize_majority_b accepted_siblings) = None.
Proof.
  split; reflexivity.
Qed.

Theorem exact_frontier_settlement_authorizes_only_the_named_source :
  let settled := settle_exact_frontier (finalize_majority_b accepted_siblings) in
  has_exact_a_tombstone settled = true /\
  has_buffered_a settled = true /\
  In SourceA (causal_sources settled) -> False.
Proof.
  simpl; intuition discriminate.
Qed.

Theorem elected_recovery_preserves_floor_and_rehomes_stale_effect :
  let settled := settle_exact_frontier (finalize_majority_b accepted_siblings) in
  exists recovered,
    publish_elected_recovery settled = Some recovered /\
    In StaleA (selected_recovery recovered) /\
    In FreshWork (selected_recovery recovered) /\
    ~ In FloorB (selected_recovery recovered) /\
    In FloorB (committed_effects recovered) /\
    In StaleA (committed_effects recovered) /\
    In FreshWork (committed_effects recovered) /\
    NoDup (selected_recovery recovered) /\
    NoDup (committed_effects recovered).
Proof.
  exists
    {| causal_sources := [SourceB];
       floor_effects := [FloorB];
       exact_rejections :=
         [{| rejected_effect := StaleA; rejected_source := SourceA |}];
       rejected_buffer := [StaleA];
       selected_recovery := [StaleA; FreshWork];
       committed_effects := [StaleA; FloorB; FreshWork] |}.
  split.
  - reflexivity.
  - split.
    + simpl; auto.
    + split.
      * simpl; auto.
      * split.
        -- simpl; intuition discriminate.
        -- split.
           ++ simpl; auto.
           ++ split.
              ** simpl; auto.
              ** split.
                 --- simpl; auto.
                 --- split.
                     +++ repeat constructor; simpl; intuition discriminate.
                     +++ repeat constructor; simpl; intuition discriminate.
Qed.

Theorem stale_sibling_recovery_end_to_end_correct :
  In SourceA (causal_sources (finalize_majority_b accepted_siblings)) /\
  publish_elected_recovery (finalize_majority_b accepted_siblings) = None /\
  let settled := settle_exact_frontier (finalize_majority_b accepted_siblings) in
  has_exact_a_tombstone settled = true /\
  has_buffered_a settled = true /\
  exists recovered,
    publish_elected_recovery settled = Some recovered /\
    selected_recovery recovered = [StaleA; FreshWork] /\
    committed_effects recovered = [StaleA; FloorB; FreshWork].
Proof.
  split.
  - simpl; auto.
  - split.
    + reflexivity.
    + simpl.
      split.
      * reflexivity.
      * split.
        -- reflexivity.
        -- eexists; repeat split; reflexivity.
Qed.

Print Assumptions stale_sibling_recovery_end_to_end_correct.
