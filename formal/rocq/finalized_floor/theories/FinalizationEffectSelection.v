From Stdlib Require Import Arith Bool Lia Lists.List.

Import ListNotations.

Definition select_projected_effects (effects projection head : nat)
  : option (nat * nat) :=
  if (effects <=? projection) && (projection <=? head)
  then Some (effects, projection)
  else None.

Definition selected_effect (bounds : nat * nat) (revision : nat) : Prop :=
  fst bounds < revision /\ revision <= snd bounds.

Definition effect_ready (revision projection : nat) : bool :=
  (0 <? revision) && (revision <=? projection).

Theorem selection_checks_complete_cursor_order :
  forall effects projection head,
    select_projected_effects effects projection head = Some (effects, projection)
    <-> effects <= projection /\ projection <= head.
Proof.
  intros effects projection head.
  unfold select_projected_effects.
  destruct ((effects <=? projection) && (projection <=? head)) eqn:Horder.
  - apply andb_true_iff in Horder as [Heffects Hprojection].
    apply Nat.leb_le in Heffects. apply Nat.leb_le in Hprojection.
    split; auto.
  - split; [discriminate |].
    intros [Heffects Hprojection].
    apply Nat.leb_le in Heffects. apply Nat.leb_le in Hprojection.
    rewrite Heffects, Hprojection in Horder. discriminate.
Qed.

Theorem selected_effects_are_projected :
  forall effects projection head bounds revision,
    select_projected_effects effects projection head = Some bounds ->
    selected_effect bounds revision ->
    effects < revision /\ revision <= projection /\ projection <= head.
Proof.
  intros effects projection head bounds revision Hselect Hin.
  unfold select_projected_effects in Hselect.
  destruct ((effects <=? projection) && (projection <=? head)) eqn:Horder;
    try discriminate.
  inversion Hselect; subst bounds.
  unfold selected_effect in Hin; simpl in Hin.
  apply andb_true_iff in Horder as [Heffects Hprojection].
  apply Nat.leb_le in Heffects. apply Nat.leb_le in Hprojection.
  lia.
Qed.

Theorem future_append_does_not_change_projected_selection :
  forall effects projection head later_head,
    effects <= projection -> projection <= head -> head <= later_head ->
    select_projected_effects effects projection later_head =
      select_projected_effects effects projection head.
Proof.
  intros effects projection head later_head Heffects Hprojection Hlater.
  assert (Hfirst : select_projected_effects effects projection head =
                   Some (effects, projection)).
  { apply selection_checks_complete_cursor_order. auto. }
  assert (Hsecond : select_projected_effects effects projection later_head =
                    Some (effects, projection)).
  { apply selection_checks_complete_cursor_order. lia. }
  now rewrite Hfirst, Hsecond.
Qed.

Theorem later_projection_preserves_selected_readiness :
  forall effects projection head later_projection bounds revision,
    select_projected_effects effects projection head = Some bounds ->
    selected_effect bounds revision -> projection <= later_projection ->
    effect_ready revision later_projection = true.
Proof.
  intros effects projection head later_projection bounds revision Hselect Hin Hlater.
  pose proof (selected_effects_are_projected _ _ _ _ _ Hselect Hin) as Hbounds.
  unfold effect_ready. apply andb_true_iff. split.
  - apply Nat.ltb_lt. lia.
  - apply Nat.leb_le. lia.
Qed.

Theorem unprojected_effect_is_not_ready :
  forall revision projection,
    projection < revision -> effect_ready revision projection = false.
Proof.
  intros revision projection Hfuture.
  unfold effect_ready.
  apply Nat.leb_gt in Hfuture. rewrite Hfuture. now rewrite andb_false_r.
Qed.

Theorem older_projected_effect_remains_ready :
  forall revision projection,
    0 < revision -> revision <= projection ->
    effect_ready revision projection = true.
Proof.
  intros revision projection Hpositive Hprojected.
  unfold effect_ready. apply andb_true_iff. split.
  - now apply Nat.ltb_lt.
  - now apply Nat.leb_le.
Qed.

Definition guarded_effect (revision projection : nat) (applied : list nat)
  : list nat * bool :=
  if effect_ready revision projection
  then (revision :: applied, true)
  else (applied, false).

Definition late_receipt_guard (revision projection : nat) (applied : list nat)
  : list nat * bool :=
  (revision :: applied, effect_ready revision projection).

Theorem rejected_early_effect_is_observationally_inert :
  forall revision projection applied,
    effect_ready revision projection = false ->
    guarded_effect revision projection applied = (applied, false).
Proof.
  intros revision projection applied Hnotready.
  unfold guarded_effect. now rewrite Hnotready.
Qed.

Theorem late_receipt_rejection_does_not_undo_an_effect :
  forall revision projection applied,
    effect_ready revision projection = false ->
    fst (late_receipt_guard revision projection applied) <> applied.
Proof.
  intros revision projection applied Hnotready.
  unfold late_receipt_guard; simpl.
  intro Hsame. apply (f_equal (@length nat)) in Hsame. simpl in Hsame. lia.
Qed.

Example fresh_head_selection_exposes_an_unprojected_round :
  selected_effect (0, 1) 1 /\ effect_ready 1 0 = false /\
  select_projected_effects 0 0 1 = Some (0, 0).
Proof.
  unfold selected_effect, effect_ready, select_projected_effects; simpl.
  repeat split; lia.
Qed.
