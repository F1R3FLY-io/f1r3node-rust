From Stdlib Require Import Bool.Bool.

Inductive LifecycleVerdict :=
| LifecyclePending
| LifecycleFinalized
| LifecycleFailed
| LifecycleExpired.

Inductive LifecycleAnchor :=
| NoLifecycleAnchor
| DeployOccurrenceCarrier
| FinalizedStateFloor.

Definition lifecycle_decision
  (effect_in_floor history_readable failed_in_floor expiry_bound_crossed : bool)
  : LifecycleVerdict :=
  if effect_in_floor then LifecycleFinalized
  else if negb history_readable then LifecyclePending
  else if failed_in_floor then LifecycleFailed
  else if expiry_bound_crossed then LifecycleExpired
  else LifecyclePending.

Definition state_bearing_lifecycle_decision
  (successful_effect_in_lfb history_readable failed_settlement_in_lfb
   expiry_bound_crossed finality_marker frozen_floor_covers_carrier : bool)
  : LifecycleVerdict :=
  lifecycle_decision successful_effect_in_lfb history_readable
    failed_settlement_in_lfb expiry_bound_crossed.

Definition restore_ready_lifecycle_decision
  (floor_ready successful_effect_in_lfb history_readable
   failed_settlement_in_lfb expiry_bound_crossed finality_marker
   frozen_floor_covers_carrier : bool)
  : LifecycleVerdict :=
  if floor_ready then
    state_bearing_lifecycle_decision successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier
  else LifecyclePending.

Definition terminal_anchor_projection
  (has_occurrence : bool) (verdict : LifecycleVerdict)
  : LifecycleAnchor * LifecycleAnchor :=
  match verdict with
  | LifecyclePending => (NoLifecycleAnchor, NoLifecycleAnchor)
  | LifecycleFinalized | LifecycleFailed =>
      (if has_occurrence then DeployOccurrenceCarrier else NoLifecycleAnchor,
       FinalizedStateFloor)
  | LifecycleExpired =>
      (if has_occurrence then DeployOccurrenceCarrier else NoLifecycleAnchor,
       FinalizedStateFloor)
  end.

Theorem successful_floor_effect_has_priority :
  forall history_readable failed_in_floor expiry_bound_crossed,
    lifecycle_decision true history_readable failed_in_floor expiry_bound_crossed =
    LifecycleFinalized.
Proof. reflexivity. Qed.

Theorem unreadable_history_abstains_without_effect :
  forall failed_in_floor expiry_bound_crossed,
    lifecycle_decision false false failed_in_floor expiry_bound_crossed =
    LifecyclePending.
Proof. reflexivity. Qed.

Theorem readable_adopted_failure_is_immediately_terminal :
  forall expiry_bound_crossed,
    lifecycle_decision false true true expiry_bound_crossed = LifecycleFailed.
Proof. reflexivity. Qed.

Theorem expiry_requires_readable_stable_absence :
  forall effect_in_floor history_readable failed_in_floor expiry_bound_crossed,
    lifecycle_decision effect_in_floor history_readable failed_in_floor
      expiry_bound_crossed = LifecycleExpired ->
    effect_in_floor = false /\
    history_readable = true /\
    failed_in_floor = false /\
    expiry_bound_crossed = true.
Proof.
  intros effect_in_floor history_readable failed_in_floor expiry_bound_crossed H.
  destruct effect_in_floor, history_readable, failed_in_floor,
    expiry_bound_crossed; discriminate H ||
    exact (conj eq_refl (conj eq_refl (conj eq_refl eq_refl))).
Qed.

Theorem later_block_is_not_a_lifecycle_premise :
  forall later_block_seen,
    lifecycle_decision false true true later_block_seen = LifecycleFailed.
Proof. intros []; reflexivity. Qed.

Theorem finality_marker_is_not_settlement_evidence :
  forall history_readable expiry_bound_crossed finality_marker
    frozen_floor_covers_carrier,
    state_bearing_lifecycle_decision false history_readable false
      expiry_bound_crossed finality_marker frozen_floor_covers_carrier =
    lifecycle_decision false history_readable false expiry_bound_crossed.
Proof. reflexivity. Qed.

Theorem frozen_floor_coverage_is_not_settlement_evidence :
  forall history_readable expiry_bound_crossed finality_marker
    frozen_floor_covers_carrier,
    state_bearing_lifecycle_decision false history_readable false
      expiry_bound_crossed finality_marker frozen_floor_covers_carrier =
    lifecycle_decision false history_readable false expiry_bound_crossed.
Proof. reflexivity. Qed.

Theorem failed_settlement_in_adopted_lfb_is_terminal :
  forall expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    state_bearing_lifecycle_decision false true true expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier = LifecycleFailed.
Proof. reflexivity. Qed.

Theorem restore_without_floor_readiness_abstains :
  forall successful_effect_in_lfb history_readable failed_settlement_in_lfb
    expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    restore_ready_lifecycle_decision false successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier = LifecyclePending.
Proof. reflexivity. Qed.

Theorem restored_floor_readiness_refines_lifecycle_decision :
  forall successful_effect_in_lfb history_readable failed_settlement_in_lfb
    expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    restore_ready_lifecycle_decision true successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier =
    state_bearing_lifecycle_decision successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier.
Proof. reflexivity. Qed.

Theorem restore_readiness_contract :
  (forall successful_effect_in_lfb history_readable failed_settlement_in_lfb
      expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    restore_ready_lifecycle_decision false successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier = LifecyclePending) /\
  (forall successful_effect_in_lfb history_readable failed_settlement_in_lfb
      expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    restore_ready_lifecycle_decision true successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier =
    state_bearing_lifecycle_decision successful_effect_in_lfb
      history_readable failed_settlement_in_lfb expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier).
Proof.
  exact
    (conj restore_without_floor_readiness_abstains
      restored_floor_readiness_refines_lifecycle_decision).
Qed.

Theorem finalized_anchor_roles_are_separate :
  terminal_anchor_projection true LifecycleFinalized =
    (DeployOccurrenceCarrier, FinalizedStateFloor).
Proof. reflexivity. Qed.

Theorem failed_anchor_roles_are_separate :
  terminal_anchor_projection true LifecycleFailed =
    (DeployOccurrenceCarrier, FinalizedStateFloor).
Proof. reflexivity. Qed.

Theorem state_floor_is_never_an_occurrence_carrier :
  forall has_occurrence verdict occurrence_anchor state_anchor,
    terminal_anchor_projection has_occurrence verdict =
      (occurrence_anchor, state_anchor) ->
    occurrence_anchor <> FinalizedStateFloor.
Proof.
  intros [] [] occurrence_anchor state_anchor H;
    inversion H; discriminate.
Qed.

Theorem deploy_lifecycle_finalization_contract :
  (forall history_readable failed_in_floor expiry_bound_crossed,
    lifecycle_decision true history_readable failed_in_floor expiry_bound_crossed =
    LifecycleFinalized) /\
  (forall expiry_bound_crossed,
    lifecycle_decision false true true expiry_bound_crossed = LifecycleFailed) /\
  terminal_anchor_projection true LifecycleFinalized =
    (DeployOccurrenceCarrier, FinalizedStateFloor) /\
  terminal_anchor_projection true LifecycleFailed =
    (DeployOccurrenceCarrier, FinalizedStateFloor) /\
  (forall history_readable expiry_bound_crossed finality_marker
      frozen_floor_covers_carrier,
    state_bearing_lifecycle_decision false history_readable false
      expiry_bound_crossed finality_marker frozen_floor_covers_carrier =
    lifecycle_decision false history_readable false expiry_bound_crossed) /\
  (forall expiry_bound_crossed finality_marker frozen_floor_covers_carrier,
    state_bearing_lifecycle_decision false true true expiry_bound_crossed
      finality_marker frozen_floor_covers_carrier = LifecycleFailed) /\
  (forall has_occurrence verdict occurrence_anchor state_anchor,
    terminal_anchor_projection has_occurrence verdict =
      (occurrence_anchor, state_anchor) ->
    occurrence_anchor <> FinalizedStateFloor).
Proof.
  exact
    (conj successful_floor_effect_has_priority
      (conj readable_adopted_failure_is_immediately_terminal
        (conj finalized_anchor_roles_are_separate
          (conj failed_anchor_roles_are_separate
            (conj finality_marker_is_not_settlement_evidence
              (conj failed_settlement_in_adopted_lfb_is_terminal
                state_floor_is_never_an_occurrence_carrier)))))).
Qed.

Print Assumptions deploy_lifecycle_finalization_contract.
Print Assumptions restore_readiness_contract.
